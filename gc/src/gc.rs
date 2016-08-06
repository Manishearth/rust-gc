use std::cell::{Cell, RefCell};
use std::mem;
use std::ptr::Shared;
use trace::Trace;

const INITIAL_THRESHOLD: usize = 100;

// after collection we want the the ratio of used/total to be no
// greater than this (the threshold grows exponentially, to avoid
// quadratic behavior when the heap is growing linearly with the
// number of `new` calls):
const USED_SPACE_RATIO: f64 = 0.7;

struct GcState {
    bytes_allocated: usize,
    threshold: usize,
    boxes_start: Option<Shared<GcBox<Trace>>>,
}

impl Drop for GcState {
    fn drop(&mut self) {
        unsafe {
            GC_SWEEPING.with(|collecting| collecting.set(true));

            while let Some(node) = self.boxes_start {
                let node = Box::from_raw(*node);
                self.boxes_start = node.header.next;
            }
        }
    }
}

/// Whether or not the thread is currently in the sweep phase of garbage collection.
/// During this phase, attempts to dereference a `Gc<T>` pointer will trigger a panic.
thread_local!(static GC_SWEEPING: Cell<bool> = Cell::new(false));

/// The garbage collector's internal state.
thread_local!(static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
    bytes_allocated: 0,
    threshold: INITIAL_THRESHOLD,
    boxes_start: None,
}));

pub struct GcBoxHeader {
    // XXX This is horribly space inefficient - not sure if we care
    // We are using a word word bool - there is a full 63 bits of unused data :(
    roots: Cell<usize>,
    next: Option<Shared<GcBox<Trace>>>,
    marked: Cell<bool>,
}

pub struct GcBox<T: Trace + ?Sized + 'static> {
    header: GcBoxHeader,
    data: T,
}

impl<T: Trace> GcBox<T> {
    /// Allocates a garbage collected `GcBox` on the heap,
    /// and appends it to the thread-local `GcBox` chain.
    ///
    /// A `GcBox` allocated this way starts its life rooted.
    pub fn new(value: T) -> Shared<Self> {
        GC_STATE.with(|st| {
            let mut st = st.borrow_mut();

            // XXX We should probably be more clever about collecting
            if st.bytes_allocated > st.threshold {
                collect_garbage(&mut *st);

                if st.bytes_allocated as f64 > st.threshold as f64 * USED_SPACE_RATIO  {
                    // we didn't collect enough, so increase the
                    // threshold for next time, to avoid thrashing the
                    // collector too much/behaving quadratically.
                    st.threshold = (st.bytes_allocated as f64 / USED_SPACE_RATIO) as usize
                }
            }

            let gcbox = unsafe {
                Shared::new(Box::into_raw(Box::new(GcBox {
                    header: GcBoxHeader {
                        roots: Cell::new(1),
                        marked: Cell::new(false),
                        next: st.boxes_start.take(),
                    },
                    data: value,
                })))
            };

            st.boxes_start = Some(gcbox);

            // We allocated some bytes! Let's record it
            st.bytes_allocated += mem::size_of::<GcBox<T>>();

            // Return the pointer to the newly allocated data
            gcbox
        })
    }
}

impl<T: Trace + ?Sized> GcBox<T> {
    /// Marks this `GcBox` and traces through its data.
    pub unsafe fn trace_inner(&self) {
        let marked = self.header.marked.get();
        if !marked {
            self.header.marked.set(true);
            self.data.trace();
        }
    }

    /// Increases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub unsafe fn root_inner(&self) {
        // abort if the count overflows to prevent `mem::forget` loops that could otherwise lead to
        // erroneous drops
        self.header.roots.set(self.header.roots.get()
                              .checked_add(1).unwrap_or_else(|| ::std::intrinsics::abort()));
    }

    /// Decreases the root count on this `GcBox`.
    /// Roots prevent the `GcBox` from being destroyed by the garbage collector.
    pub unsafe fn unroot_inner(&self) {
        self.header.roots.set(self.header.roots.get() - 1);
    }

    /// Returns a reference to the `GcBox`'s value.
    pub fn value(&self) -> &T {
        // XXX This may be too expensive, but will help catch errors with
        // accessing Gc values in destructors.
        GC_SWEEPING.with(|sweeping| assert!(!sweeping.get(),
                                            "Gc pointers may be invalid when GC is running"));
        &self.data
    }

    fn size_of(&self) -> usize { mem::size_of_val(self) }
}

/// Collects garbage.
fn collect_garbage(st: &mut GcState) {
    unsafe fn mark(mut head: Option<Shared<GcBox<Trace>>>) {
        while let Some(node) = head {
            if (**node).header.roots.get() > 0 {
                (**node).trace_inner();
            }

            head = (**node).header.next;
        }
    }

    unsafe fn sweep(mut head: &mut Option<Shared<GcBox<Trace>>>, bytes_allocated: &mut usize) {
        GC_SWEEPING.with(|collecting| collecting.set(true));

        while let Some(node) = *head {
            if (**node).header.marked.get() {
                // This node has already been marked - we're done!
                (**node).header.marked.set(false);
                head = &mut (**node).header.next;
            } else {
                // The node wasn't marked - we need to delete it
                let mut node = Box::from_raw(*node);
                *bytes_allocated -= node.size_of();
                *head = node.header.next.take();
            }
        }

        // XXX This should probably be done with some kind of finally guard
        GC_SWEEPING.with(|collecting| collecting.set(false));
    }

    unsafe {
        mark(st.boxes_start);
        sweep(&mut st.boxes_start, &mut st.bytes_allocated);
    }
}

/// Immediately triggers a garbage collection on the current thread.
pub fn force_collect() {
    GC_STATE.with(|st| {
        let mut st = st.borrow_mut();
        collect_garbage(&mut *st);
    });
}
