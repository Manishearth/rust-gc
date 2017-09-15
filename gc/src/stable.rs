//! This module contains minimal stable dummy implementations of NonZero and
//! Shared, such that the same code can be used between the nightly and stable
//! versions of rust-gc.

use std::marker::PhantomData;

/// See `::std::ptr::Shared`
pub struct Shared<T: ?Sized> {
    p: *mut T,
    _pd: PhantomData<T>,
}

impl<T: ?Sized> Shared<T> {
    pub unsafe fn new_unchecked(p: *mut T) -> Self {
        Shared {
            p: p,
            _pd: PhantomData,
        }
    }

    pub fn as_ptr(&self) -> *mut T {
        self.p
    }
}

impl<T: ?Sized> Clone for Shared<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for Shared<T> {}
