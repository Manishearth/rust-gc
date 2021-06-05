use gc::{Gc, Trace};
use gc_derive::{Finalize, Trace};

trait Foo: Trace {}

#[derive(Trace, Finalize)]
struct Bar;
impl Foo for Bar {}

#[derive(Trace, Finalize)]
struct AnyFoo(dyn Foo);

#[test]
fn gc_box_dyn_foo() {
    let _: Gc<Box<dyn Foo>> = Gc::new(Box::new(Bar));
}

#[cfg(feature = "nightly")]
#[test]
fn gc_dyn_foo() {
    let _: Gc<dyn Foo> = Gc::new(Bar);
}

#[allow(dead_code)]
fn gc_box_anyfoo(b: Box<AnyFoo>) -> Gc<Box<AnyFoo>> {
    Gc::new(b)
}
