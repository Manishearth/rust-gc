use crate::trace::Trace;
use std::cell::{Cell, RefCell};
use std::mem;
use std::ptr::NonNull;

const INITIAL_THRESHOLD: usize = 100;

// after collection we want the the ratio of used/total to be no
// greater than this (the threshold grows exponentially, to avoid
// quadratic behavior when the heap is growing linearly with the
// number of `new` calls):
const USED_SPACE_RATIO: f64 = 0.7;

struct GcState {
    bytes_allocated: usize,
    threshold: usize,
    boxes_start: Option<Box<GcBox<dyn Trace>>>,
}

impl Drop for GcState {
    fn drop(&mut self) {
        let mut head = &self.boxes_start;
        while let Some(ref node) = *head {
            node.data.finalize();
            head = &node.header.next;
        }

        // Drop all allocations in the singly-linked list.
        // This could be done with `self.boxes_start = None;`,
        // but that might lead to a large number of recursive drops.
        let _guard = DropGuard::new();
        let mut head = self.boxes_start.take();
        while let Some(node) = head {
            head = node.header.next;
        }
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
    bytes_allocated: 0,
    threshold: INITIAL_THRESHOLD,
    boxes_start: None,
}));

pub(crate) struct GcBoxHeader {
    // XXX This is horribly space inefficient - not sure if we care
    // We are using a word word bool - there is a full 63 bits of unused data :(
    // XXX: Should be able to store marked in the high bit of roots?
    roots: Cell<usize>,
    next: Option<Box<GcBox<dyn Trace>>>,
    marked: Cell<bool>,
}

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
            if st.bytes_allocated > st.threshold {
                collect_garbage(&mut *st);

                if st.bytes_allocated as f64 > st.threshold as f64 * USED_SPACE_RATIO {
                    // we didn't collect enough, so increase the
                    // threshold for next time, to avoid thrashing the
                    // collector too much/behaving quadratically.
                    st.threshold = (st.bytes_allocated as f64 / USED_SPACE_RATIO) as usize
                }
            }

            let gcbox = Box::new(GcBox {
                header: GcBoxHeader {
                    roots: Cell::new(1),
                    marked: Cell::new(false),
                    next: st.boxes_start.take(),
                },
                data: value,
            });
            let ptr = NonNull::from(&*gcbox);

            st.boxes_start = Some(gcbox);

            // We allocated some bytes! Let's record it
            st.bytes_allocated += mem::size_of::<GcBox<T>>();

            // Return the pointer to the newly allocated data
            ptr
        })
    }
}

impl<T: Trace + ?Sized> GcBox<T> {
    /// Marks this `GcBox` and marks through its data.
    pub(crate) unsafe fn trace_inner(&self) {
        let marked = self.header.marked.get();
        if !marked {
            self.header.marked.set(true);
            self.data.trace();
        }
    }

    /// Increases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub(crate) unsafe fn root_inner(&self) {
        // abort if the count overflows to prevent `mem::forget` loops that could otherwise lead to
        // erroneous drops
        self.header
            .roots
            .set(self.header.roots.get().checked_add(1).unwrap());
    }

    /// Decreases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub(crate) unsafe fn unroot_inner(&self) {
        self.header.roots.set(self.header.roots.get() - 1);
    }

    /// Returns a reference to the `GcBox`'s value.
    pub(crate) fn value(&self) -> &T {
        &self.data
    }
}

/// Collects garbage.
fn collect_garbage(st: &mut GcState) {
    unsafe fn mark(head: &Option<Box<GcBox<dyn Trace>>>) {
        // Walk the tree, tracing and marking the nodes
        let mut mark_head = head;
        while let Some(ref node) = *mark_head {
            if node.header.roots.get() > 0 {
                node.trace_inner();
            }

            mark_head = &node.header.next;
        }
    }

    unsafe fn sweep(
        head: &mut Option<Box<GcBox<dyn Trace>>>,
        bytes_allocated: &mut usize,
    ) {
        let _guard = DropGuard::new();

        // Collect the unmarked nodes from the allocation list into a vector.
        // Also unmark the nodes which were marked, to prepare for the next GC.
        let mut unmarked = None;
        let mut unmark_head = head;
        while let Some(mut node) = unmark_head.take() {
            if node.header.marked.get() {
                node.header.marked.set(false);
                // `get_or_insert()` will always re-insert `node`.
                // It is just used to get a reference to the next node pointer.
                unmark_head = &mut unmark_head.get_or_insert(node).header.next;
            } else {
                // Finalize the node's contents
                node.value().finalize_glue();
                // Move `node` from the allocation list to the unmarked list
                *unmark_head = node.header.next;
                node.header.next = unmarked;
                unmarked = Some(node);
            }
        }

        while let Some(node) = unmarked {
            *bytes_allocated -= mem::size_of_val::<GcBox<_>>(&*node);
            unmarked = node.header.next;
            // `node` is dropped here, freeing the allocation
        }
    }

    unsafe {
        mark(&st.boxes_start);
        sweep(&mut st.boxes_start, &mut st.bytes_allocated);
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
