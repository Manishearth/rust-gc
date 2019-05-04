extern crate gc;

use std::marker::PhantomData;

use gc::Gc;

enum Uninhabited {}

#[test]
fn phantom_data() {
    let _x: Gc<PhantomData<Uninhabited>> = Gc::new(PhantomData);
}
