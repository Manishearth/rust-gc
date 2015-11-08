//! Thread-local garbage-collected boxes (The `Gc<T>` type).
//!
//! The `Gc<T>` type provides shared ownership of an immutable value.
//! It is marked as non-sendable because the garbage collection only occurs
//! thread locally.

#![feature(borrow_state, core, optin_builtin_traits, nonzero)]

extern crate core;

use core::nonzero::NonZero;
use gc::GcBox;
use std::cell::{self, Cell, RefCell, BorrowState, UnsafeCell};
use std::ops::{Deref, DerefMut};
use std::marker;

mod gc;
mod trace;

// We re-export the Trace method, as well as some useful internal methods for
// managing collections or configuring the garbage collector.
pub use trace::Trace;
pub use gc::force_collect;

const EVERYTHING_BUT_LEAST_SIGNIFICANT_BIT: usize = ::std::usize::MAX - 1;

////////
// Gc //
////////

/// A garbage-collected pointer type over an immutable value.
///
/// See the [module level documentation](./) for more details.
pub struct Gc<T: Trace + ?Sized + 'static> {
    _ptr: UnsafeCell<NonZero<*mut GcBox<T>>>,
}

impl<T: ?Sized> !marker::Send for Gc<T> {}

impl<T: ?Sized> !marker::Sync for Gc<T> {}

impl<T: Trace> Gc<T> {
    /// Constructs a new `Gc<T>`.
    ///
    /// # Collection
    ///
    /// This method could trigger a Garbage Collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::Gc;
    ///
    /// let five = Gc::new(5);
    /// ```
    pub fn new(value: T) -> Gc<T> {
        unsafe {
            // Allocate the memory for the object
            let ptr = GcBox::new(value);

            // When we create a Gc<T>, all pointers which have been moved to the
            // heap no longer need to be rooted, so we unroot them.
            (**ptr).value().unroot();
            let gc = Gc { _ptr: UnsafeCell::new(ptr) };
            gc.set_rooted(true);
            gc
        }
    }
}

impl<T: Trace + ?Sized> Gc<T> {
    #[inline]
    fn inner(&self) -> &GcBox<T> {
        use std::mem::transmute;
        unsafe {
            let gc_box: *mut GcBox<_> = **self._ptr.get();
            let ptrs: *mut usize = transmute(&gc_box);
            *ptrs &= EVERYTHING_BUT_LEAST_SIGNIFICANT_BIT;
            transmute::<*mut GcBox<_>, &mut GcBox<_>>(gc_box)
        }
    }

    fn is_rooted(&self) -> bool {
        use std::mem::transmute;
        unsafe {
            let gc_box: &*mut GcBox<T> = &**self._ptr.get();
            // Address of gc_box on the stack.
            let ptrs: *mut usize = transmute::<&*mut _, _>(gc_box);
            *ptrs & 1 == 1
        }
    }

    #[allow(mutable_transmutes)]
    fn set_rooted(&self, b: bool) {
        use std::mem::transmute;
        unsafe {
            let gc_box: &*mut GcBox<T> = &**self._ptr.get();
            // Address of gc_box on the stack.
            let ptrs: *mut usize = transmute::<&*mut _, _>(gc_box);

            // Clear out the bottom bit
            *ptrs = *ptrs & EVERYTHING_BUT_LEAST_SIGNIFICANT_BIT;
            // Set the bottom bit
            *ptrs = *ptrs | if b { 1 } else { 0 };
        }
    }
}

unsafe impl<T: Trace + ?Sized> Trace for Gc<T> {
    #[inline]
    unsafe fn trace(&self) {
        self.inner().trace_inner();
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.is_rooted(), "Can't double-root a Gc<T>");
        self.set_rooted(true);

        self.inner().root_inner();
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.is_rooted(), "Can't double-unroot a Gc<T>");
        self.set_rooted(false);

        self.inner().unroot_inner();
    }
}

impl<T: Trace + ?Sized> Clone for Gc<T> {
    #[inline]
    fn clone(&self) -> Gc<T> {
        unsafe { self.inner().root_inner(); }
        let gc = unsafe {
            Gc { _ptr: UnsafeCell::new(*self._ptr.get())}
        };
        gc.set_rooted(true);
        gc
    }
}

impl<T: Trace + ?Sized> Deref for Gc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner().value()
    }
}

impl<T: Trace + ?Sized> Drop for Gc<T> {
    #[inline]
    fn drop(&mut self) {
        // If this pointer was a root, we should unroot it.
        if self.is_rooted() {
            unsafe { self.inner().unroot_inner(); }
        }
    }
}

////////////
// GcCell //
////////////

/// A mutable memory location with dynamically checked borrow rules
/// which can be used inside of a garbage collected pointer.
///
/// This object is a RefCell which can be used inside of a Gc<T>.
pub struct GcCell<T: ?Sized + 'static> {
    rooted: Cell<bool>,
    cell: RefCell<T>,
}

impl <T: Trace> GcCell<T> {
    /// Creates a new `GcCell` containing `value`.
    #[inline]
    pub fn new(value: T) -> GcCell<T> {
        GcCell {
            rooted: Cell::new(true),
            cell: RefCell::new(value),
        }
    }

    /// Consumes the `GcCell`, returning the wrapped value.
    #[inline]
    pub fn into_inner(self) -> T {
        self.cell.into_inner()
    }
}

impl <T: Trace + ?Sized> GcCell<T> {
    /// Immutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned `GcCellRef` exits scope.
    /// Multiple immutable borrows can be taken out at the same time.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently mutably borrowed.
    #[inline]
    pub fn borrow(&self) -> GcCellRef<T> {
        self.cell.borrow()
    }

    /// Mutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned `GcCellRefMut` exits scope.
    /// The value cannot be borrowed while this borrow is active.
    ///
    /// #Panics
    ///
    /// Panics if the value is currently borrowed.
    #[inline]
    pub fn borrow_mut(&self) -> GcCellRefMut<T> {
        let val_ref = self.cell.borrow_mut();

        // Force the val_ref's contents to be rooted for the duration of the mutable borrow
        if !self.rooted.get() {
            unsafe { val_ref.root(); }
        }

        GcCellRefMut {
            _ref: val_ref,
            _rooted: &self.rooted,
        }
    }
}

unsafe impl<T: Trace + ?Sized> Trace for GcCell<T> {
    #[inline]
    unsafe fn trace(&self) {
        match self.cell.borrow_state() {
            BorrowState::Writing => (),
            _ => self.cell.borrow().trace(),
        }
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.rooted.get(), "Can't root a GcCell Twice!");
        self.rooted.set(true);

        match self.cell.borrow_state() {
            BorrowState::Writing => (),
            _ => self.cell.borrow().root(),
        }
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.rooted.get(), "Can't unroot a GcCell Twice!");
        self.rooted.set(false);

        match self.cell.borrow_state() {
            BorrowState::Writing => (),
            _ => self.cell.borrow().unroot(),
        }
    }
}

/// A wrapper type for an immutably borrowed value from a GcCell<T>
pub type GcCellRef<'a, T> = cell::Ref<'a, T>;

/// A wrapper type for a mutably borrowed value from a GcCell<T>
pub struct GcCellRefMut<'a, T: Trace + ?Sized + 'static> {
    _ref: ::std::cell::RefMut<'a, T>,
    _rooted: &'a Cell<bool>,
}

impl<'a, T: Trace + ?Sized> Deref for GcCellRefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T { &*self._ref }
}

impl<'a, T: Trace + ?Sized> DerefMut for GcCellRefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T { &mut *self._ref }
}

impl<'a, T: Trace + ?Sized> Drop for GcCellRefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        // Restore the rooted state of the GcCell's contents to the state of the GcCell.
        // During the lifetime of the GcCellRefMut, the GcCell's contents are rooted.
        if !self._rooted.get() {
            unsafe { self._ref.unroot(); }
        }
    }
}
