#![cfg_attr(feature = "nightly", feature(i128_type))]

extern crate gc;

#[allow(unused_imports)]
use gc::Gc;

#[cfg(feature = "nightly")]
#[test]
fn i128() {
    Gc::new(0i128);
}

#[cfg(feature = "nightly")]
#[test]
fn u128() {
    Gc::new(0u128);
}
