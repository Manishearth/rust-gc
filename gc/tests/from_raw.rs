use gc::Gc;

#[test]
fn test_into_raw() {
    let x = Gc::new(22);
    let x_ptr = Gc::into_raw(x);
    let x = unsafe { Gc::from_raw(x_ptr) };
    let y = Gc::new(x);
    assert_eq!(**y, 22);
}
