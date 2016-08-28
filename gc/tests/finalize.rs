#![feature(plugin, custom_derive, specialization)]

#![plugin(gc_plugin)]
extern crate gc;

use std::cell::Cell;
use gc::Finalize;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
struct Flags(i32, i32);

#[derive(Trace)]
struct A {
    b: B,
}

#[derive(Trace)]
struct B;

thread_local!(static FLAGS: Cell<Flags> = Cell::new(Flags(0, 0)));

impl Finalize for A {
    fn finalize(&self) {
        FLAGS.with(|f| {
            let mut of = f.get();
            of.0 += 1;
            f.set(of);
        });
    }
}

impl Finalize for B {
    fn finalize(&self) {
        FLAGS.with(|f| {
            let mut of = f.get();
            of.1 += 1;
            f.set(of);
        });
    }
}

#[test]
fn drop_triggers_finalize() {
    FLAGS.with(|f| assert_eq!(f.get(), Flags(0, 0)));
    {
        let _x = A { b: B };
        FLAGS.with(|f| assert_eq!(f.get(), Flags(0, 0)));
    }
    FLAGS.with(|f| assert_eq!(f.get(), Flags(1, 1)));
}
