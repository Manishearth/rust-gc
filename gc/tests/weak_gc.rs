use gc::{Gc, WeakGc, GcCell};

#[test]
fn weak_gc_try_deref_some_value() {
    let weak = WeakGc::new(GcCell::new(1));
    let comparable = GcCell::new(1);
    assert_eq!(weak.try_deref(), Some(&comparable));
}

#[test]
fn weak_gc_from_existing() {
    let gc = Gc::new(GcCell::new(1));
    let weak_gc = gc.clone_weak_ref();
    let comparable = GcCell::new(1);
    assert_eq!(weak_gc.try_deref(), Some(&comparable))
}