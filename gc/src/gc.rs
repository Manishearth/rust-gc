use crate::trace::Trace;
use std::cell::{Cell, RefCell};
use std::mem;
use std::ptr::{self, NonNull};

struct GcState {
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
    stats: GcStats::default(),
    config: GcConfig::default(),
    boxes_start: Cell::new(None),
}));

const MARK_MASK: usize = 1 << (usize::BITS - 1);
const ROOTS_MASK: usize = !MARK_MASK;
const ROOTS_MAX: usize = ROOTS_MASK; // max allowed value of roots

pub(crate) struct GcBoxHeader {
    roots: Cell<usize>, // high bit is used as mark flag
    next: Cell<Option<NonNull<GcBox<dyn Trace>>>>,
}

impl GcBoxHeader {
    #[inline]
    pub fn new(next: Option<NonNull<GcBox<dyn Trace>>>) -> Self {
        GcBoxHeader {
            roots: Cell::new(1), // unmarked and roots count = 1
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
}

#[repr(C)] // to justify the layout computation in Gc::from_raw
pub(crate) struct GcBox<T: Trace + ?Sized + 'static> {
    header: GcBoxHeader,
    data: T,
}

impl<T: Trace> GcBox<T> {
    /// Allocates a garbage collected `GcBox` on the heap,
    /// and appends it to the thread-local `GcBox` chain.
    ///
    /// A `GcBox` allocated this way starts its life rooted.
    pub(crate) fn new(value: T) -> NonNull<Self> {
        GC_STATE.with(|st| {
            let mut st = st.borrow_mut();

            // XXX We should probably be more clever about collecting
            if st.stats.bytes_allocated > st.config.threshold {
                collect_garbage(&mut st);

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

            let gcbox = Box::into_raw(Box::new(GcBox {
                header: GcBoxHeader::new(st.boxes_start.take()),
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

impl<T: Trace + ?Sized> GcBox<T> {
    /// Returns `true` if the two references refer to the same `GcBox`.
    pub(crate) fn ptr_eq(this: &GcBox<T>, other: &GcBox<T>) -> bool {
        // Use .header to ignore fat pointer vtables, to work around
        // https://github.com/rust-lang/rust/issues/46139
        ptr::eq(&this.header, &other.header)
    }

    /// Marks this `GcBox` and marks through its data.
    pub(crate) unsafe fn trace_inner(&self) {
        if !self.header.is_marked() {
            self.header.mark();
            self.data.trace();
        }
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
}

/// Collects garbage.
fn collect_garbage(st: &mut GcState) {
    struct Unmarked<'a> {
        incoming: &'a Cell<Option<NonNull<GcBox<dyn Trace>>>>,
        this: NonNull<GcBox<dyn Trace>>,
    }
    unsafe fn mark(head: &Cell<Option<NonNull<GcBox<dyn Trace>>>>) -> Vec<Unmarked<'_>> {
        // Walk the tree, tracing and marking the nodes
        let mut mark_head = head.get();
        while let Some(node) = mark_head {
            if (*node.as_ptr()).header.roots() > 0 {
                (*node.as_ptr()).trace_inner();
            }

            mark_head = (*node.as_ptr()).header.next.get();
        }

        // Collect a vector of all of the nodes which were not marked,
        // and unmark the ones which were.
        let mut unmarked = Vec::new();
        let mut unmark_head = head;
        while let Some(node) = unmark_head.get() {
            if (*node.as_ptr()).header.is_marked() {
                (*node.as_ptr()).header.unmark();
            } else {
                unmarked.push(Unmarked {
                    incoming: unmark_head,
                    this: node,
                });
            }
            unmark_head = &(*node.as_ptr()).header.next;
        }
        unmarked
    }

    unsafe fn sweep(finalized: Vec<Unmarked<'_>>, bytes_allocated: &mut usize) {
        let _guard = DropGuard::new();
        for node in finalized.into_iter().rev() {
            if (*node.this.as_ptr()).header.is_marked() {
                continue;
            }
            let incoming = node.incoming;
            let node = Box::from_raw(node.this.as_ptr());
            *bytes_allocated -= mem::size_of_val::<GcBox<_>>(&*node);
            incoming.set(node.header.next.take());
        }
    }

    st.stats.collections_performed += 1;

    unsafe {
        let unmarked = mark(&st.boxes_start);
        if unmarked.is_empty() {
            return;
        }
        for node in &unmarked {
            Trace::finalize_glue(&(*node.this.as_ptr()).data);
        }
        mark(&st.boxes_start);
        sweep(unmarked, &mut st.stats.bytes_allocated);
    }
}

/// Immediately triggers a garbage collection on the current thread.
///
/// This will panic if executed while a collection is currently in progress
pub fn force_collect() {
    GC_STATE.with(|st| {
        let mut st = st.borrow_mut();
        collect_garbage(&mut st);
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

#[derive(Clone, Default)]
pub struct GcStats {
    pub bytes_allocated: usize,
    pub collections_performed: usize,
}

#[allow(dead_code)]
pub fn stats() -> GcStats {
    GC_STATE.with(|st| st.borrow().stats.clone())
}
