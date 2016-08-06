use std::cell::{Cell, RefCell};
use std::mem;
use std::ptr::{self, Shared};
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
    boxes_start: Option<Box<GcBox<Trace + 'static>>>,
    boxes_end: *mut Option<Box<GcBox<Trace + 'static>>>,
}

/// Whether or not the thread is currently in the sweep phase of garbage collection.
/// During this phase, attempts to dereference a `Gc<T>` pointer will trigger a panic.
thread_local!(static GC_SWEEPING: Cell<bool> = Cell::new(false));

/// The garbage collector's internal state.
thread_local!(static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
    bytes_allocated: 0,
    threshold: INITIAL_THRESHOLD,
    boxes_start: None,
    boxes_end: ptr::null_mut(),
}));

pub struct GcBoxHeader {
    // XXX This is horribly space inefficient - not sure if we care
    // We are using a word word bool - there is a full 63 bits of unused data :(
    roots: Cell<usize>,
    next: Option<Box<GcBox<Trace + 'static>>>,
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
        GC_STATE.with(|_st| {
            let mut st = _st.borrow_mut();

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

            let mut gcbox = Box::new(GcBox {
                header: GcBoxHeader {
                    roots: Cell::new(1),
                    marked: Cell::new(false),
                    next: None,
                },
                data: value,
            });

            let gcbox_ptr = unsafe { Shared::new(&mut *gcbox as *mut _) };

            let next_boxes_end = &mut gcbox.header.next as *mut _;
            if st.boxes_end.is_null() {
                assert!(st.boxes_start.is_none(),
                        "If something had been allocated, boxes_end would be set");
                // The next place we're going to add something!
                st.boxes_end = next_boxes_end;
                st.boxes_start = Some(gcbox);
            } else {
                unsafe {
                    *st.boxes_end = Some(gcbox);
                }
                st.boxes_end = next_boxes_end;
            }

            // We allocated some bytes! Let's record it
            st.bytes_allocated += mem::size_of::<GcBox<T>>();

            // Return the pointer to the newly allocated data
            gcbox_ptr
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

    fn header_mut(&mut self) -> &mut GcBoxHeader {
        &mut self.header
    }

    fn size_of(&self) -> usize { mem::size_of_val(self) }
}

/// Collects garbage.
fn collect_garbage(st: &mut GcState) {
    let mut next_node = &mut st.boxes_start
        as *mut Option<Box<GcBox<Trace + 'static>>>;

    // Mark
    while let Some(ref mut node) = *unsafe { &mut *next_node } {
        {
            let header = node.header_mut();
            next_node = &mut header.next as *mut _;

            // If it doesn't have roots - we can abort now
            if header.roots.get() == 0 { continue }
        }
        // We trace in a different scope such that node isn't
        // mutably borrowed anymore
        unsafe { node.trace_inner(); }
    }

    GC_SWEEPING.with(|collecting| collecting.set(true));

    let mut next_node = &mut st.boxes_start
        as *mut Option<Box<GcBox<Trace + 'static>>>;

    // Sweep
    while let Some(ref mut node) = *unsafe { &mut *next_node } {
        let size = node.size_of();
        let header = node.header_mut();

        if header.marked.get() {
            // This node has already been marked - we're done!
            header.marked.set(false);
            next_node = &mut header.next;
        } else {
            // The node wasn't marked - we need to delete it
            st.bytes_allocated -= size;
            let mut tmp = None;
            mem::swap(&mut tmp, &mut header.next);
            mem::swap(&mut tmp, unsafe { &mut *next_node });

            // At this point, the node is destroyed if it exists due to tmp dropping
        }
    }

    // Update the end pointer to point to the correct location
    st.boxes_end = next_node;

    // XXX This should probably be done with some kind of finally guard
    GC_SWEEPING.with(|collecting| collecting.set(false));
}

/// Immediately triggers a garbage collection on the current thread.
pub fn force_collect() {
    GC_STATE.with(|st| {
        let mut st = st.borrow_mut();
        collect_garbage(&mut *st);
    });
}
