use crate::trace::Trace;
use crate::weak::Ephemeron;
use crate::Finalize;
use std::cell::{Cell, RefCell};
use std::mem;
use std::ptr::{self, NonNull};

pub(crate) struct GcState {
    stats: GcStats,
    config: GcConfig,
    boxes_start: Cell<Option<NonNull<GcBox<dyn Trace>>>>,
}

impl Drop for GcState {
    fn drop(&mut self) {
        if !self.config.leak_on_drop {
            collect_garbage(self);
        }
        // We have no choice but to leak any remaining nodes that
        // might be referenced from other thread-local variables.
    }
}

// Whether or not the thread is currently in the sweep phase of garbage collection.
// During this phase, attempts to dereference a `Gc<T>` pointer will trigger a panic.
thread_local!(pub static GC_DROPPING: Cell<bool> = Cell::new(false));
struct DropGuard;
impl DropGuard {
    fn new() -> DropGuard {
        GC_DROPPING.with(|dropping| dropping.set(true));
        DropGuard
    }
}
impl Drop for DropGuard {
    fn drop(&mut self) {
        GC_DROPPING.with(|dropping| dropping.set(false));
    }
}
pub fn finalizer_safe() -> bool {
    GC_DROPPING.with(|dropping| !dropping.get())
}

// The garbage collector's internal state.
thread_local!(static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
    stats: Default::default(),
    config: Default::default(),
    boxes_start: Cell::new(None),
}));

pub enum GcBoxType {
    Standard,
    Weak,
    Ephemeron,
}

const MARK_MASK: usize = 1 << (usize::BITS - 1);
const ROOTS_MASK: usize = !MARK_MASK;
const ROOTS_MAX: usize = ROOTS_MASK; // max allowed value of roots

pub(crate) struct GcBoxHeader {
    roots: Cell<usize>, // high bit is used as mark flag
    ephemeron_flag: Cell<bool>,
    next: Cell<Option<NonNull<GcBox<dyn Trace>>>>,
}

impl GcBoxHeader {
    #[inline]
    pub fn new(next: Option<NonNull<GcBox<dyn Trace>>>) -> Self {
        GcBoxHeader {
            roots: Cell::new(1), // unmarked and roots count = 1
            ephemeron_flag: Cell::new(false),
            next: Cell::new(next),
        }
    }

    #[inline]
    pub fn new_ephemeron(next: Option<NonNull<GcBox<dyn Trace>>>) -> Self {
        GcBoxHeader {
            roots: Cell::new(0),
            ephemeron_flag: Cell::new(true),
            next: Cell::new(next),
        }
    }

    #[inline]
    pub fn new_weak(next: Option<NonNull<GcBox<dyn Trace>>>) -> Self {
        GcBoxHeader {
            roots: Cell::new(0),
            ephemeron_flag: Cell::new(false),
            next: Cell::new(next),
        }
    }

    #[inline]
    pub fn roots(&self) -> usize {
        self.roots.get() & ROOTS_MASK
    }

    #[inline]
    pub fn inc_roots(&self) {
        let roots = self.roots.get();

        // abort if the count overflows to prevent `mem::forget` loops
        // that could otherwise lead to erroneous drops
        if (roots & ROOTS_MASK) < ROOTS_MAX {
            self.roots.set(roots + 1); // we checked that this wont affect the high bit
        } else {
            panic!("roots counter overflow");
        }
    }

    #[inline]
    pub fn dec_roots(&self) {
        self.roots.set(self.roots.get() - 1) // no underflow check
    }

    #[inline]
    pub fn is_marked(&self) -> bool {
        self.roots.get() & MARK_MASK != 0
    }

    #[inline]
    pub fn mark(&self) {
        self.roots.set(self.roots.get() | MARK_MASK)
    }

    #[inline]
    pub fn unmark(&self) {
        self.roots.set(self.roots.get() & !MARK_MASK)
    }

    #[inline]
    pub fn is_ephemeron(&self) -> bool {
        self.ephemeron_flag.get()
    }
}

#[repr(C)] // to justify the layout computation in Gc::from_raw
pub struct GcBox<T: Trace + ?Sized + 'static> {
    header: GcBoxHeader,
    data: T,
}

impl<T: Trace + ?Sized> GcBox<T> {
    /// Returns `true` if the two references refer to the same `GcBox`.
    pub(crate) fn ptr_eq(this: &GcBox<T>, other: &GcBox<T>) -> bool {
        // Use .header to ignore fat pointer vtables, to work around
        // https://github.com/rust-lang/rust/issues/46139
        ptr::eq(&this.header, &other.header)
    }

    /// Marks this `GcBox` and marks through its data.
    pub(crate) unsafe fn trace_inner(&self) {
        if !self.header.is_marked() && !self.header.is_ephemeron() {
            self.header.mark();
            self.data.trace();
        }
    }

    /// Trace inner data
    pub(crate) unsafe fn weak_trace_inner(&self, queue: &mut Vec<NonNull<GcBox<dyn Trace>>>) {
        self.data.weak_trace(queue);
    }

    /// Increases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub(crate) unsafe fn root_inner(&self) {
        self.header.inc_roots();
    }

    /// Decreases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub(crate) unsafe fn unroot_inner(&self) {
        self.header.dec_roots();
    }

    /// Returns a pointer to the `GcBox`'s value, without dereferencing it.
    pub(crate) fn value_ptr(this: *const GcBox<T>) -> *const T {
        unsafe { ptr::addr_of!((*this).data) }
    }

    /// Returns a reference to the `GcBox`'s value.
    pub(crate) fn value(&self) -> &T {
        &self.data
    }

    pub(crate) fn is_marked(&self) -> bool {
        self.header.is_marked()
    }
}

impl<T: Trace> GcBox<T> {
    /// Allocates a garbage collected `GcBox` on the heap,
    /// and appends it to the thread-local `GcBox` chain.
    ///
    /// A `GcBox` allocated this way starts its life rooted.
    pub(crate) fn new(value: T, box_type: GcBoxType) -> NonNull<Self> {
        GC_STATE.with(|st| {
            let mut st = st.borrow_mut();

            // XXX We should probably be more clever about collecting
            if st.stats.bytes_allocated > st.config.threshold {
                collect_garbage(&mut *st);

                if st.stats.bytes_allocated as f64
                    > st.config.threshold as f64 * st.config.used_space_ratio
                {
                    // we didn't collect enough, so increase the
                    // threshold for next time, to avoid thrashing the
                    // collector too much/behaving quadratically.
                    st.config.threshold =
                        (st.stats.bytes_allocated as f64 / st.config.used_space_ratio) as usize
                }
            }

            let header = match box_type {
                GcBoxType::Standard => GcBoxHeader::new(st.boxes_start.take()),
                GcBoxType::Weak => GcBoxHeader::new_weak(st.boxes_start.take()),
                GcBoxType::Ephemeron => GcBoxHeader::new_ephemeron(st.boxes_start.take()),
            };

            let gcbox = Box::into_raw(Box::new(GcBox {
                header,
                data: value,
            }));

            st.boxes_start
                .set(Some(unsafe { NonNull::new_unchecked(gcbox) }));

            // We allocated some bytes! Let's record it
            st.stats.bytes_allocated += mem::size_of::<GcBox<T>>();

            // Return the pointer to the newly allocated data
            unsafe { NonNull::new_unchecked(gcbox) }
        })
    }
}

/// Collects garbage.
fn collect_garbage(st: &mut GcState) {
    st.stats.collections_performed += 1;

    unsafe fn mark(
        head: &Cell<Option<NonNull<GcBox<dyn Trace>>>>,
    ) -> Vec<NonNull<GcBox<dyn Trace>>> {
        // Walk the tree, tracing and marking the nodes
        let mut finalize = Vec::new();
        let mut ephemeron_queue = Vec::new();
        let mut mark_head = head;
        while let Some(node) = mark_head.get() {
            if (*node.as_ptr()).header.is_ephemeron() {
                ephemeron_queue.push(node);
            } else {
                if (*node.as_ptr()).header.roots() > 0 {
                    (*node.as_ptr()).trace_inner();
                } else {
                    finalize.push(node)
                }
            }
            mark_head = &(*node.as_ptr()).header.next;
        }

        // Ephemeron Evaluation
        if !ephemeron_queue.is_empty() {
            loop {
                let mut reachable_nodes = Vec::new();
                let mut other_nodes = Vec::new();
                // iterate through ephemeron queue, sorting nodes by whether they
                // are reachable or unreachable<?>
                for node in ephemeron_queue {
                    if (*node.as_ptr()).data.is_marked_ephemeron() {
                        (*node.as_ptr()).header.mark();
                        reachable_nodes.push(node);
                    } else {
                        other_nodes.push(node);
                    }
                }
                // Replace the old queue with the unreachable<?>
                ephemeron_queue = other_nodes;

                // If reachable nodes is not empty, trace values. If it is empty,
                // break from the loop
                if !reachable_nodes.is_empty() {
                    // iterate through reachable nodes and trace their values,
                    // enqueuing any ephemeron that is found during the trace
                    for node in reachable_nodes {
                        (*node.as_ptr()).weak_trace_inner(&mut ephemeron_queue)
                    }
                } else {
                    break;
                }
            }
        }

        // Any left over nodes in the ephemeron queue at this point are
        // unreachable and need to be notified/finalized.
        finalize.extend(ephemeron_queue);

        finalize
    }

    unsafe fn finalize(finalize_vec: Vec<NonNull<GcBox<dyn Trace>>>) {
        for node in finalize_vec {
            // We double check that the unreachable nodes are actually unreachable
            // prior to finalization as they could have been marked by a different
            // trace after initially being added to the queue
            if !(*node.as_ptr()).header.is_marked() {
                Finalize::finalize(&(*node.as_ptr()).data)
            }
        }
    }

    unsafe fn sweep(head: &Cell<Option<NonNull<GcBox<dyn Trace>>>>, bytes_allocated: &mut usize) {
        let _guard = DropGuard::new();

        let mut sweep_head = head;
        while let Some(node) = sweep_head.get() {
            if (*node.as_ptr()).header.is_marked() {
                (*node.as_ptr()).header.unmark();
                sweep_head = &(*node.as_ptr()).header.next;
            } else {
                let unmarked_node = Box::from_raw(node.as_ptr());
                *bytes_allocated -= mem::size_of_val::<GcBox<_>>(&*unmarked_node);
                sweep_head.set(unmarked_node.header.next.take());
            }
        }
    }

    unsafe {
        // Run mark and return vector of nonreachable porperties
        let unreachable_nodes = mark(&st.boxes_start);
        // Finalize the unreachable properties
        finalize(unreachable_nodes);
        // Run mark again to mark any nodes that are resurrected by their finalizer
        //
        // At this point, _f should be filled with all nodes that are unreachable and
        // have already been finalized, so they can be ignored.
        let _f = mark(&st.boxes_start);
        // Run sweep: unmarking all marked nodes and freeing any unmarked nodes
        sweep(&st.boxes_start, &mut st.stats.bytes_allocated);
    }
}

/// Immediately triggers a garbage collection on the current thread.
///
/// This will panic if executed while a collection is currently in progress
pub fn force_collect() {
    GC_STATE.with(|st| {
        let mut st = st.borrow_mut();
        collect_garbage(&mut *st);
    });
}

pub struct GcConfig {
    pub threshold: usize,
    /// after collection we want the the ratio of used/total to be no
    /// greater than this (the threshold grows exponentially, to avoid
    /// quadratic behavior when the heap is growing linearly with the
    /// number of `new` calls):
    pub used_space_ratio: f64,
    /// For short-running processes it is not always appropriate to run
    /// GC, sometimes it is better to let system free the resources
    pub leak_on_drop: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            used_space_ratio: 0.7,
            threshold: 100,
            leak_on_drop: false,
        }
    }
}

#[allow(dead_code)]
pub fn configure(configurer: impl FnOnce(&mut GcConfig)) {
    GC_STATE.with(|st| {
        let mut st = st.borrow_mut();
        configurer(&mut st.config);
    })
}

#[derive(Clone)]
pub struct GcStats {
    pub bytes_allocated: usize,
    pub collections_performed: usize,
}

impl Default for GcStats {
    fn default() -> Self {
        Self {
            bytes_allocated: 0,
            collections_performed: 0,
        }
    }
}

#[allow(dead_code)]
pub fn stats() -> GcStats {
    GC_STATE.with(|st| st.borrow().stats.clone())
}
