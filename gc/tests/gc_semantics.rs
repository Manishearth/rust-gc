#![feature(plugin, custom_derive)]

#![plugin(gc_plugin)]
extern crate gc;

use std::cell::Cell;
use std::thread::LocalKey;
use gc::{Trace, GcCell, Gc, gc_force_collect};

// Utility methods for the tests
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
struct GcWatchFlags {
    trace: i32,
    root: i32,
    unroot: i32,
    drop: i32,
}

impl GcWatchFlags {
    fn new(trace: i32, root: i32, unroot: i32, drop: i32) -> GcWatchFlags {
        GcWatchFlags {
            trace: trace,
            root: root,
            unroot: unroot,
            drop: drop,
        }
    }

    fn zero() -> Cell<GcWatchFlags> {
        Cell::new(GcWatchFlags {
            trace: 0,
            root: 0,
            unroot: 0,
            drop: 0,
        })
    }
}

struct GcWatch(&'static LocalKey<Cell<GcWatchFlags>>);

impl Drop for GcWatch {
    fn drop(&mut self) {
        self.0.with(|f| {
            let mut of = f.get();
            of.drop += 1;
            f.set(of);
        });
    }
}

impl Trace for GcWatch {
    unsafe fn _trace<T: gc::Tracer>(&self, _: T) {
        unimplemented!();
    }
    unsafe fn _gc_mark(&self) {
        self.0.with(|f| {
            let mut of = f.get();
            of.trace += 1;
            f.set(of);
        });
    }
    unsafe fn _gc_root(&self) {
        self.0.with(|f| {
            let mut of = f.get();
            of.root += 1;
            f.set(of);
        });
    }
    unsafe fn _gc_unroot(&self) {
        self.0.with(|f| {
            let mut of = f.get();
            of.unroot += 1;
            f.set(of);
        });
    }
}

#[derive(Trace)]
struct GcWatchCycle {
    watch: GcWatch,
    cycle: GcCell<Option<Gc<GcWatchCycle>>>,
}

// Tests

#[test]
fn basic_allocate() {
    thread_local!(static FLAGS: Cell<GcWatchFlags> = GcWatchFlags::zero());

    {
        let _gced_val = Gc::new(GcWatch(&FLAGS));
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 1, 0)));
        gc_force_collect();
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));
    }

    FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));
    gc_force_collect();
    FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 1)));
}

#[test]
fn basic_cycle_allocate() {
    thread_local!(static FLAGS1: Cell<GcWatchFlags> = GcWatchFlags::zero());
    thread_local!(static FLAGS2: Cell<GcWatchFlags> = GcWatchFlags::zero());

    {
        // Set up 2 nodes
        let node1 = Gc::new(GcWatchCycle {
            watch: GcWatch(&FLAGS1),
            cycle: GcCell::new(None),
        });
        FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 1, 0)));
        let node2 = Gc::new(GcWatchCycle {
            watch: GcWatch(&FLAGS2),
            cycle: GcCell::new(Some(node1.clone())),
        });

        FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 1, 0)));
        FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 1, 0)));

        gc_force_collect();

        FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));
        FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));

        // Move node2 into the cycleref
        {
            *node1.cycle.borrow_mut() = Some(node2);

            FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));
            FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));

            gc_force_collect();

            FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));
            FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));
        }

        FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));
        FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));

        gc_force_collect();

        FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 0)));
        FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 0)));
    }

    FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 0)));
    FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 0)));

    gc_force_collect();

    FLAGS1.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 1)));
    FLAGS2.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 0, 1, 1)));
}

#[test]
fn gccell_rooting() {
    thread_local!(static FLAGS: Cell<GcWatchFlags> = GcWatchFlags::zero());

    {
        let cell = GcCell::new(GcWatch(&FLAGS));

        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));

        {
            // Borrow it
            let _borrowed = cell.borrow();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));

            // Shared borrows can happen multiple times in one scope
            let _borrowed2 = cell.borrow();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));
        }

        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));

        {
            // Borrow it mutably now
            let _borrowed = cell.borrow_mut();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));
        }

        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 0, 0)));

        // Put it in a gc (should unroot the GcWatch)
        let gc_wrapper = Gc::new(cell);
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(0, 0, 1, 0)));

        // It should be traced by the GC
        gc_force_collect();
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));

        {
            // Borrow it
            let _borrowed = gc_wrapper.borrow();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));

            // Shared borrows can happen multiple times in one scope
            let _borrowed2 = gc_wrapper.borrow();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(1, 0, 1, 0)));

            // It should be traced by the GC
            gc_force_collect();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));
        }

        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 0, 1, 0)));

        {
            // Borrow it mutably now - this should root the GcWatch
            let _borrowed = gc_wrapper.borrow_mut();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 1, 1, 0)));

            // It shouldn't be traced by the GC (as it's owned by the GcCell)
            // If it had rootable members, they would be traced by the GC
            gc_force_collect();
            FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 1, 1, 0)));
        }

        // Dropping the borrow should unroot it again
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(2, 1, 2, 0)));

        // It should be traced by the GC
        gc_force_collect();
        FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 1, 2, 0)))
    }

    // It should be collected by the GC
    gc_force_collect();
    FLAGS.with(|f| assert_eq!(f.get(), GcWatchFlags::new(3, 1, 2, 1)))
}

/*
// XXX Broken by new changes to Trace (method _trace has generic type parameters)
// XXX Object Safety - FIXME

#[test]
fn trait_gc() {
    #[derive(Trace)]
    struct Bar;
    trait Foo: Trace { fn f(&self) -> i32; }
    impl Foo for Bar {
        fn f(&self) -> i32 { 10 }
    }
    fn use_trait_gc(x: Gc<Foo>) {
        assert_eq!(x.f(), 10);
    }

    let gc_bar = Gc::new(Bar);
    let gc_foo: Gc<Foo> = gc_bar.clone();

    use_trait_gc(gc_foo);
    use_trait_gc(gc_bar);
}
*/
