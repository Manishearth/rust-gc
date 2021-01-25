use gc::{Gc, GcCell, GcCellRefMut};

#[test]
fn test_gc_cell_ref_mut_map() {
    let a = Gc::new(GcCell::new((0, Gc::new(1))));
    *GcCellRefMut::map(a.borrow_mut(), |(n, _)| n) = 2;
    assert_eq!(a.borrow_mut().0, 2);
}
