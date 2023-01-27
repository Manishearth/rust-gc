#![cfg(feature = "nightly")]
#![feature(arbitrary_self_types)]

use gc::{Finalize, Gc, Trace};

trait Foo: Trace {
    fn foo(self: Gc<Self>);
}

#[derive(Trace, Finalize)]
struct Bar;

impl Foo for Bar {
    fn foo(self: Gc<Bar>) {}
}

#[test]
fn gc_self_method() {
    let gc: Gc<dyn Foo> = Gc::new(Bar);
    gc.foo();
}
