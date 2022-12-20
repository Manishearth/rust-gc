use gc::{Finalize, Gc, Trace};

trait Foo: Trace {}

#[derive(Trace, Finalize)]
struct Bar;
impl Foo for Bar {}

#[test]
fn test_from_box_sized() {
    let b: Box<[i32; 3]> = Box::new([1, 2, 3]);
    let _: Gc<[i32; 3]> = Gc::from(b);
}

#[cfg(feature = "nightly")]
#[test]
fn test_from_box_dyn() {
    let b: Box<dyn Foo> = Box::new(Bar);
    let _: Gc<dyn Foo> = Gc::from(b);
}
