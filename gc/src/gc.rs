use std::ptr;
use std::mem;
use std::cell::{Cell, RefCell};
use trace::Trace;

// XXX Obviously not 100 bytes GC threshold - choose a number
const GC_THRESHOLD: usize = 100;

struct GcState {
    bytes_allocated: usize,
    boxes_start: Option<Box<GcBoxTrait + 'static>>,
    boxes_end: *mut Option<Box<GcBoxTrait + 'static>>,
}

thread_local!(static GC_SWEEPING: Cell<bool> = Cell::new(false));
thread_local!(static GC_STATE: RefCell<GcState> = RefCell::new(GcState {
    bytes_allocated: 0,
    boxes_start: None,
    boxes_end: ptr::null_mut(),
}));

pub struct GcBoxHeader {
    // XXX This is horribly space inefficient - not sure if we care
    // We are using a word word bool - there is a full 63 bits of unused data :(
    roots: Cell<usize>,
    next: Option<Box<GcBoxTrait + 'static>>,
    marked: Cell<bool>,
}

pub trait GcBoxTrait {
    /// Get a reference to the internal GcBoxHeader
    fn header(&self) -> &GcBoxHeader;
    fn header_mut(&mut self) -> &mut GcBoxHeader;

    /// Mark this GcBox, and trace through it's data
    ///
    /// This method is unsafe because incorrect use
    /// could cause visible references to be freed.
    unsafe fn trace_inner(&self);

    /// Increase the root count on this GcBox.
    /// Roots prevent the GcBox from being destroyed by
    /// the garbage collector.
    ///
    /// This method is unsafe because incorrect use
    /// could cause visible references to be freed.
    unsafe fn root_inner(&self);

    /// Decrease the root count on this GcBox.
    /// Roots prevent the GcBox from being destroyed by
    /// the garbage collector.
    ///
    /// This method is unsafe because incorrect use
    /// could cause visible references to be freed.
    unsafe fn unroot_inner(&self);
}

pub struct GcBox<T: Trace + 'static> {
    header: GcBoxHeader,
    data: T,
}

impl<T: Trace> GcBoxTrait for GcBox<T> {
    fn header(&self) -> &GcBoxHeader { &self.header }

    fn header_mut(&mut self) -> &mut GcBoxHeader { &mut self.header }

    unsafe fn trace_inner(&self) {
        let marked = self.header.marked.get();
        if !marked {
            self.header.marked.set(true);
            self.data.trace();
        }
    }

    unsafe fn root_inner(&self) {
        self.header.roots.set(self.header.roots.get() + 1);
    }

    unsafe fn unroot_inner(&self) {
        self.header.roots.set(self.header.roots.get() - 1);
    }
}

impl<T: Trace> GcBox<T> {
    ///
    /// Allocate a garbage collected GcBox on the heap,
    /// and append it to the thread local GcBox chain.
    ///
    /// The GcBox allocated this way starts it's life
    /// rooted.
    ///
    /// # NOTE
    /// This method could trigger a collection.
    ///
    pub fn new(value: T) -> *mut GcBox<T> {
        GC_STATE.with(|_st| {
            let mut st = _st.borrow_mut();

            // XXX We should probably be more clever about collecting
            if st.bytes_allocated > GC_THRESHOLD {
                collect_garbage(&mut *st);
            }

            let mut gcbox = Box::new(GcBox {
                header: GcBoxHeader {
                    roots: Cell::new(1),
                    marked: Cell::new(false),
                    next: None,
                },
                data: value,
            });

            let gcbox_ptr = &mut *gcbox as *mut _;

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

    /// Get the value form the GcBox
    pub fn value(&self) -> &T {
        // XXX This may be too expensive, but will help catch errors with
        // accessing Gc values in destructors.
        GC_SWEEPING.with(|sweeping| assert!(!sweeping.get(),
                                            "Gc pointers may be invalid when GC is running"));
        &self.data
    }
}

/// Collect some garbage!
fn collect_garbage(st: &mut GcState) {
    let mut next_node = &mut st.boxes_start
        as *mut Option<Box<GcBoxTrait + 'static>>;

    // Mark
    loop {
        if let Some(ref mut node) = *unsafe { &mut *next_node } {
            {
                // XXX This virtual method call is nasty :(
                let header = node.header_mut();
                next_node = &mut header.next as *mut _;

                // If it doesn't have roots - we can abort now
                if header.roots.get() == 0 { continue }
            }
            // We trace in a different scope such that node isn't
            // mutably borrowed anymore
            unsafe { node.trace_inner(); }
        } else { break }
    }

    GC_SWEEPING.with(|collecting| collecting.set(true));

    let mut next_node = &mut st.boxes_start
        as *mut Option<Box<GcBoxTrait + 'static>>;

    // Sweep
    loop {
        if let Some(ref mut node) = *unsafe { &mut *next_node } {
            // XXX This virtual method call is nasty :(
            let header = node.header_mut();

            if header.marked.get() {
                // This node has already been marked - we're done!
                header.marked.set(false);
                next_node = &mut header.next;
            } else {
                // The node wasn't marked - we need to delete it
                let mut tmp = None;
                mem::swap(&mut tmp, &mut header.next);
                mem::swap(&mut tmp, unsafe { &mut *next_node });

                // At this point, the node is destroyed if it exists due to tmp dropping
            }
        } else { break }
    }

    // XXX This should probably be done with some kind of finally guard
    GC_SWEEPING.with(|collecting| collecting.set(false));
}

/// Forcibly collects the current thread's garbage
pub fn force_collect() {
    GC_STATE.with(|_st| {
        let mut st = _st.borrow_mut();
        collect_garbage(&mut *st);
    });
}
