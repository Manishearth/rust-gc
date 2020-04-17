#![cfg_attr(feature = "nightly", feature(i128_type))]

extern crate gc;

use gc::Gc;

#[test]
fn i128() {
    Gc::new(0i128);
}

#[test]
fn u128() {
    Gc::new(0u128);
}
