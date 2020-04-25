#![cfg_attr(feature = "nightly", feature(specialization))]

use gc_derive::{Finalize, Trace};
use std::cell::RefCell;

thread_local!(static X: RefCell<u8> = RefCell::new(0));

use gc::Trace;

#[derive(Copy, Clone, Finalize)]
struct Foo;

unsafe impl Trace for Foo {
    unsafe fn trace(&self) {
        X.with(|x| {
            let mut m = x.borrow_mut();
            *m += 1;
        })
    }
    unsafe fn root(&self) {}
    unsafe fn unroot(&self) {}
    fn finalize_glue(&self) {}
}

#[derive(Trace, Clone, Finalize)]
struct Bar {
    inner: Foo,
}

#[derive(Trace, Finalize)]
struct Baz {
    a: Bar,
    b: Bar,
}

#[derive(Trace, Finalize, Copy, Clone)]
struct CopyTrace {
    inner: Foo,
}

#[test]
fn test() {
    let bar = Bar { inner: Foo };
    unsafe {
        bar.trace();
    }
    X.with(|x| assert!(*x.borrow() == 1));
    let baz = Baz {
        a: bar.clone(),
        b: bar.clone(),
    };
    unsafe {
        baz.trace();
    }
    X.with(|x| assert!(*x.borrow() == 3));

    let copytrace = CopyTrace { inner: Foo };
    unsafe {
        copytrace.trace();
    }
    X.with(|x| assert!(*x.borrow() == 4));
}
