#![cfg_attr(feature = "nightly", feature(specialization))]

use gc_derive::{Finalize, Trace};
use std::cell::RefCell;
use std::rc::Rc;

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

#[derive(Trace, Clone, Finalize)]
struct InnerBoxSlice {
    inner: Box<[u32]>,
}

#[derive(Trace, Clone, Finalize)]
struct InnerBoxStr {
    inner: Box<str>,
}

#[derive(Trace, Clone, Finalize)]
struct InnerRcSlice {
    inner: Box<[u32]>,
}

#[derive(Trace, Clone, Finalize)]
struct InnerRcStr {
    inner: Rc<str>,
}

#[derive(Trace, Clone, Finalize)]
struct InnerRcStruct {
    inner: Rc<Bar>,
}

#[derive(Trace, Finalize)]
struct Baz {
    a: Bar,
    b: Bar,
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
}
