use gc::{Finalize, Gc, Trace};

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

#[test]
fn gc_box_slice() {
    let _: Gc<Box<[u32]>> = Gc::new(Box::new([0, 1, 2]));
}

#[cfg(feature = "nightly")]
#[test]
fn gc_slice() {
    let _: Gc<[u32]> = Gc::new([0, 1, 2]);
}

#[test]
fn gc_box_str() {
    let _: Gc<Box<str>> = Gc::new(Box::from("hello"));
}

#[cfg(feature = "nightly")]
#[allow(dead_code)]
fn gc_str(_: Gc<str>) {
    // no way to construct this yet
}
