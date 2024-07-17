use gc::Gc;
#[cfg(feature = "nightly")]
use gc::{Finalize, Trace};

#[cfg(feature = "nightly")]
trait Foo: Trace {}

#[cfg(feature = "nightly")]
#[derive(Trace, Finalize)]
struct Bar;
#[cfg(feature = "nightly")]
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
