#![feature(plugin, custom_derive, test)]

#![plugin(gc_plugin)]
extern crate gc;

extern crate test;

use std::sync::atomic::{AtomicUsize, ATOMIC_USIZE_INIT, Ordering};
use gc::{Trace, Cgc, cgc_force_collect};

// Helper method for comparing the fields of GcWatchFlags instances
macro_rules! assert_gcwf {
    ($flags:expr, $root:expr, $unroot:expr, $drop:expr) => {
        {
            let flgs = & $flags;
            let got = (flgs.root.load(Ordering::Relaxed),
                       flgs.unroot.load(Ordering::Relaxed),
                       flgs.drop.load(Ordering::Relaxed));
            let expected = ($root, $unroot, $drop);

            assert_eq!(got, expected);
        }
    }
}

// Utility methods for the tests
struct GcWatchFlags {
    root: AtomicUsize,
    unroot: AtomicUsize,
    drop: AtomicUsize,
}

const GC_WATCH_FLAGS_INIT: GcWatchFlags = GcWatchFlags {
    root: ATOMIC_USIZE_INIT,
    unroot: ATOMIC_USIZE_INIT,
    drop: ATOMIC_USIZE_INIT,
};

struct GcWatch(&'static GcWatchFlags);

impl Drop for GcWatch {
    fn drop(&mut self) {
        self.0.drop.fetch_add(1, Ordering::SeqCst);
    }
}

impl Trace for GcWatch {
    unsafe fn _trace<T: gc::Tracer>(&self, _: T) {
        unimplemented!();
    }
    unsafe fn _cgc_mark(&self, _: bool) {
        // As multiple tests can be running at the same time,
        // mark events can happen at times when we wouldn't expect.
        //
        // It is pretty meaningless to measure mark events, as
        // they are non-deterministic.
    }
    unsafe fn _cgc_root(&self) {
        self.0.root.fetch_add(1, Ordering::SeqCst);
    }
    unsafe fn _cgc_unroot(&self) {
        self.0.unroot.fetch_add(1, Ordering::SeqCst);
    }
}

// Tests

#[test]
fn basic_allocate() {
    static FLAGS: GcWatchFlags = GC_WATCH_FLAGS_INIT;

    {
        let _gced_val = Cgc::new(GcWatch(&FLAGS));
        assert_gcwf!(FLAGS, 0, 1, 0);
        cgc_force_collect();
        assert_gcwf!(FLAGS, 0, 1, 0);
    }

    // A collection could have happened on a seperate thread here
    cgc_force_collect();
    assert_gcwf!(FLAGS, 0, 1, 1);
}

// XXX FIXME more tests
