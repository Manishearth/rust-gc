#![cfg_attr(feature = "nightly", feature(specialization))]

#[macro_use]
extern crate gc_derive;
extern crate gc;

use gc::Finalize;
use std::cell::Cell;

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

#[derive(Trace, Finalize)]
struct X(Box<dyn Trace>);

#[test]
fn drop_triggers_finalize() {
    FLAGS.with(|f| assert_eq!(f.get(), Flags(0, 0)));
    {
        let _x = A { b: B };
        FLAGS.with(|f| assert_eq!(f.get(), Flags(0, 0)));
    }
    FLAGS.with(|f| assert_eq!(f.get(), Flags(1, 1)));
}
