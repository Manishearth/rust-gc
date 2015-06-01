//! Concurrently garbage-collected boxes (The `Cgc<T>` type).
//!
//! The `Cgc<T>` type provides shared ownership of an immutable value.
//! Unlike `Gc<T>`, `Cgc<T>` can be sent across threads, because collection
//! occurs in a thread-safe way.

use std::cell::Cell;
use std::ops::{Deref, CoerceUnsized};
use std::marker;
use cgc_internals::GcBox;
use trace::{Trace, Tracer};

// We expose the force_collect method from the gc internals
pub use cgc_internals::force_collect;

/////////
// Cgc //
/////////

/// A garbage-collected pointer type over an immutable value.
///
/// See the [module level documentation](./) for more details.
pub struct Cgc<T: Trace + ?Sized + 'static> {
    // XXX We can probably take advantage of alignment to store this
    root: Cell<bool>,
    _ptr: *mut GcBox<T>,
}

impl<T: Trace + ?Sized + marker::Unsize<U>, U: Trace + ?Sized> CoerceUnsized<Cgc<U>> for Cgc<T> {}

impl<T: Trace + Send + Sync> Cgc<T> {
    /// Constructs a new `Cgc<T>`.
    ///
    /// # Collection
    ///
    /// This method could trigger a Garbage Collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::Cgc;
    ///
    /// let five = Cgc::new(5);
    /// ```
    pub fn new(value: T) -> Cgc<T> {
        unsafe {
            // Allocate the memory for the object
            let ptr = GcBox::new(value);

            // When we create a Cgc<T>, all pointers which have been moved to the
            // heap no longer need to be rooted, so we unroot them.
            (*ptr).value()._cgc_unroot();
            Cgc { _ptr: ptr, root: Cell::new(true) }
        }
    }
}

impl<T: Trace + ?Sized> Cgc<T> {
    #[inline]
    fn inner(&self) -> &GcBox<T> {
        unsafe { &*self._ptr }
    }
}

impl<T: Trace + ?Sized> Trace for Cgc<T> {
    #[inline]
    unsafe fn _trace<U: Tracer>(&self, _: U) { /* do nothing */ }

    #[inline]
    unsafe fn _cgc_mark(&self, mark: bool) {
        self.inner().mark(mark);
    }

    #[inline]
    unsafe fn _cgc_root(&self) {
        assert!(!self.root.get(), "Can't double-root a Cgc<T>");
        self.root.set(true);

        self.inner().root();
    }

    #[inline]
    unsafe fn _cgc_unroot(&self) {
        assert!(self.root.get(), "Can't double-unroot a Cgc<T>");
        self.root.set(false);

        self.inner().unroot();
    }
}

impl<T: Trace + ?Sized> Clone for Cgc<T> {
    #[inline]
    fn clone(&self) -> Cgc<T> {
        unsafe { self.inner().root(); }
        Cgc { _ptr: self._ptr, root: Cell::new(true) }
    }
}

impl<T: Trace + ?Sized> Deref for Cgc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner().value()
    }
}

impl<T: Trace + ?Sized> Drop for Cgc<T> {
    #[inline]
    fn drop(&mut self) {
        // If this pointer was a root, we should unroot it.
        if self.root.get() {
            unsafe { self.inner().unroot(); }
        }
    }
}
