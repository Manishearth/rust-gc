use gc::{Gc, GcCell, WeakGc};

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

#[test]
fn weak_gc_different_copies() {
    let gc = Gc::new(GcCell::new(1));
    let weak_gc1 = gc.clone_weak_ref();
    let weak_gc2 = weak_gc1.clone();

    {
        let _weak_gc3 = WeakGc::new(GcCell::new(2));
        gc::force_collect();
    }

    assert_eq!(weak_gc2.has_strong_refs(), true);
}
