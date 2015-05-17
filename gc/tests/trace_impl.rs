#![feature(plugin, custom_derive)]

#![plugin(gc_plugin)]
extern crate gc;
use std::cell::RefCell;

thread_local!(static X: RefCell<u8> = RefCell::new(0));

use gc::trace::Trace;

#[derive(Copy, Clone)]
struct Foo;

impl Trace for Foo {
    fn trace(&self) {
        X.with(|x| {
            let mut m = x.borrow_mut();
            *m = *m + 1;
        })
    }
    fn root(&self){}
    fn unroot(&self){}
}

#[derive(Trace, Copy, Clone)]
struct Bar {
    inner: Foo,
}

#[derive(Trace)]
struct Baz {
    a: Bar,
    b: Bar,
}

#[test]
fn test() {
    let bar = Bar{inner: Foo};
    bar.trace();
    X.with(|x| {
        assert!(*x.borrow() == 1)
    });
    let baz = Baz {
        a: bar,
        b: bar
    };
    baz.trace();
    X.with(|x| {
        assert!(*x.borrow() == 3)
    });
}
