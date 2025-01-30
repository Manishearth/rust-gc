use gc::{Finalize, Gc, GcCell, Trace};
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
        let _x = X(Box::new(A { b: B }));
        FLAGS.with(|f| assert_eq!(f.get(), Flags(0, 0)));
    }
    FLAGS.with(|f| assert_eq!(f.get(), Flags(1, 1)));
}

#[derive(Trace)]
struct Ressurection {
    escape: Gc<GcCell<Gc<String>>>,
    value: Gc<String>,
}

impl Finalize for Ressurection {
    fn finalize(&self) {
        *self.escape.borrow_mut() = self.value.clone();
    }
}

// run this with miri to detect UB
// cargo +nightly miri test -p gc --test finalize
#[test]
fn finalizer_can_ressurect() {
    let escape = Gc::new(GcCell::new(Gc::new(String::new())));
    let value = Gc::new(GcCell::new(Ressurection {
        escape: escape.clone(),
        value: Gc::new(String::from("Hello world")),
    }));
    drop(value);

    gc::force_collect();

    assert_eq!(&**escape.borrow(), "Hello world");
}
