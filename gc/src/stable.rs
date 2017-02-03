//! This module contains minimal stable dummy implementations of NonZero and
//! Shared, such that the same code can be used between the nightly and stable
//! versions of rust-gc.

use std::ops::Deref;
use std::marker::PhantomData;

/// See `::core::nonzero::NonZero`
#[derive(Copy, Clone)]
pub struct NonZero<T> {
    p: T,
}

impl<T> Deref for NonZero<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.p
    }
}

impl<T> NonZero<T> {
    pub unsafe fn new(p: T) -> NonZero<T> {
        NonZero { p: p }
    }
}

/// See `::std::prt::Shared`
pub struct Shared<T: ?Sized> {
    p: NonZero<*mut T>,
    _pd: PhantomData<T>,
}

impl<T: ?Sized> Shared<T> {
    pub unsafe fn new(p: *mut T) -> Self {
        Shared {
            p: NonZero::new(p),
            _pd: PhantomData,
        }
    }
}

impl<T: ?Sized> Deref for Shared<T> {
    type Target = *mut T;
    fn deref(&self) -> &*mut T {
        &self.p
    }
}

impl<T: ?Sized> Clone for Shared<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for Shared<T> {}
