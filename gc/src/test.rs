use super::{Gc, GcCell, Trace, force_collect};

#[test]
#[should_panic]
fn test_issue36() {
    struct Foo(GcCell<Option<Gc<Trace>>>);

    unsafe impl Trace for Foo {
        unsafe fn trace(&self) {
            self.0.trace();
        }

        unsafe fn root(&self) {
            self.0.root();
        }

        unsafe fn unroot(&self) {
            self.0.unroot();
        }
    }

    impl Drop for Foo {
        fn drop(&mut self) {
            self.0.borrow().clone();
        }
    }

    {
        let a = Gc::new(Foo(GcCell::new(None)));
        let b = Gc::new(1);
        *a.0.borrow_mut() = Some(b);
    }

    force_collect();
}

#[test]
#[should_panic]
fn test_issue37() {
    struct Foo(GcCell<Option<Gc<Trace>>>);

    unsafe impl Trace for Foo {
        unsafe fn trace(&self) {
            self.0.trace();
        }

        unsafe fn root(&self) {
            self.0.root();
        }

        unsafe fn unroot(&self) {
            self.0.unroot();
        }
    }

    impl Drop for Foo {
        fn drop(&mut self) {
            self.0.borrow_mut();
        }
    }

    {
        let a = Gc::new(Foo(GcCell::new(None)));
        let b = Gc::new(1);
        *a.0.borrow_mut() = Some(b);
    }

    force_collect();
}

