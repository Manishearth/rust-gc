use std::mem;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering, ATOMIC_USIZE_INIT, ATOMIC_BOOL_INIT};
use std::sync::{RwLock, Mutex, TryLockError};
use std::sync::mpsc::{channel, Sender, Receiver};
use trace::Trace;

// XXX Obviously not 100 bytes GC threshold - choose a number
const GC_THRESHOLD: usize = 100;

/// The current usage of the heap
static GC_HEAP_USAGE: AtomicUsize = ATOMIC_USIZE_INIT;

/// When this value is true, newly created objects should be marked `true`,
/// and values should be sent to senders.1.
/// When this value is false, newly created objects should be marked `false`,
/// and values should be sent to senders.0.
static GC_CHANNEL: AtomicBool = ATOMIC_BOOL_INIT;

/// True if the GC is currently sweeping. When this is true, attempts to
/// dereference gc-ed pointers will panic!
static GC_SWEEPING: AtomicBool = ATOMIC_BOOL_INIT;

struct GcBoxChans {
    /// This is held before we modify roots, to ensure that we don't
    /// modify these roots during the garbage collection process.
    /// It will be held when the garbage collector is running
    rootlock: RwLock<()>,
    senders: Mutex<(Sender<Box<GcBoxTrait + Send + Sync + 'static>>,
                    Sender<Box<GcBoxTrait + Send + Sync + 'static>>)>,

    // XXX We only access when we hold the write lock on rootlock
    // We could probably use an unsafe system for this, which could save
    // us the extra overhead of the second mutex lock.
    // XXX OPTIMIZE
    receivers: Mutex<(Receiver<Box<GcBoxTrait + Send + Sync + 'static>>,
                      Receiver<Box<GcBoxTrait + Send + Sync + 'static>>)>,
}

/// The GCBOX channel queue
lazy_static! {
    static ref GCBOX_CHANS: GcBoxChans = {
        let (txa, rxa) = channel();
        let (txb, rxb) = channel();

        GcBoxChans {
            senders: Mutex::new((txa, txb)),
            receivers: Mutex::new((rxa, rxb)),
            rootlock: RwLock::new(()),
        }
    };
}

/// Thread-local cache of the senders
thread_local! {
    static GCBOX_SENDERS: (Sender<Box<GcBoxTrait + Send + Sync + 'static>>,
                           Sender<Box<GcBoxTrait + Send + Sync + 'static>>)
        = GCBOX_CHANS.senders.lock().unwrap().clone()
}

struct GcBoxHeader {
    roots: AtomicUsize,
    marked: AtomicBool,
}

/// Internal trait - must be implemented by every garbage collected allocation
/// GcBoxTraits form a linked list of allocations.
trait GcBoxTrait {
    /// Get a reference to the internal GcBoxHeader
    fn header(&self) -> &GcBoxHeader;

    /// Initiate a trace through the GcBoxTrait
    unsafe fn mark_value(&self, mark: bool);

    /// Get the size of the allocationr required to create the GcBox
    fn size_of(&self) -> usize;
}

pub struct GcBox<T: Trace + ?Sized + 'static> {
    header: GcBoxHeader,
    data: T,
}

impl<T: Trace + Send + Sync> GcBox<T> {
    /// Allocate a garbage collected GcBox on the heap,
    ///
    /// The GcBox allocated this way starts it's life rooted.
    pub fn new(value: T) -> *mut GcBox<T> {
        // Check if we should collect!
        let usage = GC_HEAP_USAGE.fetch_add(mem::size_of::<GcBox<T>>(), Ordering::SeqCst);

        if usage > GC_THRESHOLD {
            collect_garbage();
        }

        GCBOX_SENDERS.with(|senders| {
            let chan_sel = GC_CHANNEL.load(Ordering::SeqCst);

            // Copy the data onto the heap
            let mut gcbox = Box::new(GcBox {
                header: GcBoxHeader {
                    roots: AtomicUsize::new(1),
                    marked: AtomicBool::new(chan_sel),
                },
                data: value,
            });
            let ptr: *mut _ = &mut *gcbox;

            // Save the gcbox on the gc queue
            //
            // There is a chance that chan_sel has changed by now, this chance
            // is very low, and if it has, then the worst that will happen is
            // that the newly allocated object will miss the next collection
            // cycle, and only be collected in the cycle after that.
            if chan_sel {
                senders.1.send(gcbox).unwrap();
            } else {
                senders.0.send(gcbox).unwrap();
            }

            ptr
        })
    }
}

impl<T: Trace + ?Sized> GcBox<T> {
    /// Mark this GcBox, and trace through it's data
    pub unsafe fn mark(&self, mark: bool) {
        // Mark this node
        let marked = self.header.marked.swap(mark, Ordering::Relaxed);

        // If we weren't already marked, trace through child nodes
        if marked != mark { self.data._cgc_mark(mark); }
    }

    /// Increase the root count on this GcBox.
    /// Roots prevent the GcBox from being destroyed by
    /// the garbage collector.
    pub unsafe fn root(&self) {
        // XXX we may be able to avoid blocking here in some cases
        let _modifyroots_ok = GCBOX_CHANS.rootlock.read();
        self.header.roots.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrease the root count on this GcBox.
    /// Roots prevent the GcBox from being destroyed by
    /// the garbage collector.
    pub unsafe fn unroot(&self) {
        // XXX we may be able to avoid blocking here in some cases
        let _modifyroots_ok = GCBOX_CHANS.rootlock.read();
        self.header.roots.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get the value form the GcBox
    pub fn value(&self) -> &T {
        assert!(!GC_SWEEPING.load(Ordering::Relaxed),
                "Gc pointers may be invalid when Gc is running, don't deref Gc pointers in drop()");

        &self.data
    }
}

impl<T: Trace + Send + Sync> GcBoxTrait for GcBox<T> {
    fn header(&self) -> &GcBoxHeader { &self.header }

    unsafe fn mark_value(&self, mark: bool) { self.mark(mark) }

    fn size_of(&self) -> usize { mem::size_of::<T>() }
}

/// Collect some garbage!
fn collect_garbage() -> bool {
    // Try and gain access to the garbage collecting lock -
    match GCBOX_CHANS.rootlock.try_write() {
        Ok(_) => {
            // This is only locked when the write block from GCBOX_CHANS.rootlock,
            // so the mutex is unnecessary. Unfortunately, as the Receivers inside
            // the mutex don't implement sync, we can't put them directly inside of
            // the RwLock, so instead we are acquiring them in a seperate lock.
            //
            // It may make sense to do some unsafe code here to avoid this extra lock.
            let receivs = GCBOX_CHANS.receivers.lock().unwrap();

            // Toggle GC_CHANNEL - after this point, nothing more will be added
            // to the input queue
            let old_chan_sel = GC_CHANNEL.fetch_xor(true, Ordering::SeqCst);

            GCBOX_SENDERS.with(|sends| {
                let (in_chan, out_chan) = if old_chan_sel {
                    (&receivs.1, &sends.0)
                } else {
                    (&receivs.0, &sends.1)
                };

                let mut sweep_list = Vec::new();

                // Mark items off - if they are marked, we can already
                // put them on the out_chan for the next garbage collection
                loop {
                    match in_chan.try_recv() {
                        Ok(gcbox) => {
                            let (roots, marked) = {
                                let header = gcbox.header();

                                (header.roots.load(Ordering::Relaxed),
                                 header.marked.load(Ordering::Relaxed))
                            };

                            if roots > 0 {
                                unsafe { gcbox.mark_value(!old_chan_sel); }
                                out_chan.send(gcbox).unwrap();
                            } else {
                                if marked == old_chan_sel {
                                    // This may not be marked - add it to sweep_list
                                    sweep_list.push(gcbox);
                                } else {
                                    // Already marked - just send it
                                    out_chan.send(gcbox).unwrap();
                                }
                            }
                        }
                        Err(_) => break
                    }
                }

                // Go through the remaining nodes and send them on the channel if
                // they are marked.  If they are not, drop them.
                for gcbox in sweep_list {
                    if gcbox.header().marked.load(Ordering::Relaxed) != old_chan_sel {
                        out_chan.send(gcbox).unwrap();
                    } else {
                        drop(gcbox);
                    }
                }
            });

            true
        }
        Err(TryLockError::Poisoned(_)) =>
            panic!("The garbage collector lock is poisoned"),
        Err(TryLockError::WouldBlock) => false,
    }
}

/// Immediately trigger a garbage collection
pub fn force_collect() {
    // XXX: We want to always collect garbage, no matter what.
    // otherwise, running force_collect doesn't guarantee that previously
    // unrooted values in the current thread will be collected as we expect.
    // Currently, we may not actually collect garbage when we run force_collect

    if !collect_garbage() {
        println!("Already Collecting Garbage!");
        let _read = GCBOX_CHANS.rootlock.read().unwrap();
    }
}
