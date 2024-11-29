use gc::{force_collect, Finalize, Gc, GcCell};
use gc_derive::{Finalize, Trace};

#[derive(Finalize, Trace)]
struct Foo {
    bar: GcCell<Option<Gc<Bar>>>,
}

#[derive(Trace)]
struct Bar {
    string: String,
    foo: Gc<Foo>,
    this: GcCell<Option<Gc<Bar>>>,
}

impl Finalize for Bar {
    fn finalize(&self) {
        *self.foo.bar.borrow_mut() = self.this.borrow().clone();
    }
}

#[test]
fn resurrection_by_finalizer() {
    let foo = Gc::new(Foo {
        bar: GcCell::new(None),
    });
    let bar = Gc::new(Bar {
        string: "Hello, world!".to_string(),
        foo: foo.clone(),
        this: GcCell::new(None),
    });
    *bar.this.borrow_mut() = Some(bar.clone());
    drop(bar);
    force_collect();
    assert_eq!(foo.bar.borrow().as_ref().unwrap().string, "Hello, world!");
}
