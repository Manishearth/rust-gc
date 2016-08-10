//! Thread-local garbage-collected boxes (The `Gc<T>` type).
//!
//! The `Gc<T>` type provides shared ownership of an immutable value.
//! It is marked as non-sendable because the garbage collection only occurs
//! thread-locally.

#![feature(borrow_state, coerce_unsized, core_intrinsics, optin_builtin_traits, shared, unsize)]

use gc::GcBox;
use std::cell::{self, BorrowState, Cell, RefCell};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker;
use std::ops::{CoerceUnsized, Deref, DerefMut};
use std::ptr::Shared;

mod gc;
pub mod trace;

#[cfg(test)]
mod test;

// We re-export the Trace method, as well as some useful internal methods for
// managing collections or configuring the garbage collector.
pub use trace::Trace;
pub use gc::force_collect;

////////
// Gc //
////////

/// A garbage-collected pointer type over an immutable value.
///
/// See the [module level documentation](./) for more details.
pub struct Gc<T: Trace + ?Sized + 'static> {
    // XXX We can probably take advantage of alignment to store this
    root: Cell<bool>,
    ptr: Shared<GcBox<T>>,
}

impl<T: ?Sized> !Send for Gc<T> {}

impl<T: ?Sized> !Sync for Gc<T> {}

impl<T: Trace + ?Sized + marker::Unsize<U>, U: Trace + ?Sized> CoerceUnsized<Gc<U>> for Gc<T> {}

impl<T: Trace> Gc<T> {
    /// Constructs a new `Gc<T>` with the given value.
    ///
    /// # Collection
    ///
    /// This method could trigger a garbage collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::Gc;
    ///
    /// let five = Gc::new(5);
    /// ```
    pub fn new(value: T) -> Self {
        unsafe {
            // Allocate the memory for the object
            let ptr = GcBox::new(value);

            // When we create a Gc<T>, all pointers which have been moved to the
            // heap no longer need to be rooted, so we unroot them.
            (**ptr).value().unroot();
            Gc { ptr: ptr, root: Cell::new(true) }
        }
    }
}

impl<T: Trace + ?Sized> Gc<T> {
    #[inline]
    fn inner(&self) -> &GcBox<T> {
        // The GcBox behind this pointer might not be valid anymore if the GC
        // is running, so we panic, instead of accessing the potentially
        // invalid ptr.
        gc::GC_SWEEPING.with(|sweeping| {
            assert!(!sweeping.get(),
                    "Cannot access GC-ed objects while GC is running");
        });
        unsafe { &**self.ptr }
    }
}

unsafe impl<T: Trace + ?Sized> Trace for Gc<T> {
    #[inline]
    unsafe fn trace(&self) {
        self.inner().trace_inner();
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.root.get(), "Can't double-root a Gc<T>");

        // Try to get inner before modifying our state. Inner may be
        // inaccessible due to this method being invoked during the sweeping
        // phase, and we don't want to modify our state before panicking.
        self.inner().root_inner();

        self.root.set(true);
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.root.get(), "Can't double-unroot a Gc<T>");

        // Try to get inner before modifying our state. Inner may be
        // inaccessible due to this method being invoked during the sweeping
        // phase, and we don't want to modify our state before panicking.
        self.inner().unroot_inner();

        self.root.set(false);
    }
}

impl<T: Trace + ?Sized> Clone for Gc<T> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe { self.inner().root_inner(); }
        Gc { ptr: self.ptr, root: Cell::new(true) }
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
        if self.root.get() {
            unsafe { self.inner().unroot_inner(); }
        }
    }
}

impl<T: Trace + Default> Default for Gc<T> {
    #[inline]
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: Trace + ?Sized + PartialEq> PartialEq for Gc<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }

    #[inline(always)]
    fn ne(&self, other: &Self) -> bool {
        **self != **other
    }
}

impl<T: Trace + ?Sized + Eq> Eq for Gc<T> {}

impl<T: Trace + ?Sized + PartialOrd> PartialOrd for Gc<T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }

    #[inline(always)]
    fn lt(&self, other: &Self) -> bool {
        **self < **other
    }

    #[inline(always)]
    fn le(&self, other: &Self) -> bool {
        **self <= **other
    }

    #[inline(always)]
    fn gt(&self, other: &Self) -> bool {
        **self > **other
    }

    #[inline(always)]
    fn ge(&self, other: &Self) -> bool {
        **self >= **other
    }
}

impl<T: Trace + ?Sized + Ord> Ord for Gc<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: Trace + ?Sized + Hash> Hash for Gc<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: Trace + ?Sized + fmt::Display> fmt::Display for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized + fmt::Debug> fmt::Debug for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: Trace> fmt::Pointer for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&*self.ptr, f)
    }
}

impl<T: Trace> From<T> for Gc<T> {
    fn from(t: T) -> Self {
        Self::new(t)
    }
}

////////////
// GcCell //
////////////

/// A mutable memory location with dynamically checked borrow rules
/// that can be used inside of a garbage-collected pointer.
///
/// This object is a `RefCell` that can be used inside of a `Gc<T>`.
pub struct GcCell<T: ?Sized + 'static> {
    rooted: Cell<bool>,
    cell: RefCell<T>,
}

impl<T: Trace> GcCell<T> {
    /// Creates a new `GcCell` containing `value`.
    #[inline]
    pub fn new(value: T) -> Self {
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

impl<T: Trace + ?Sized> GcCell<T> {
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
    /// # Panics
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
            refm: val_ref,
            rooted: &self.rooted,
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
        assert!(!self.rooted.get(), "Can't root a GcCell twice!");
        self.rooted.set(true);

        match self.cell.borrow_state() {
            BorrowState::Writing => (),
            _ => self.cell.borrow().root(),
        }
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.rooted.get(), "Can't unroot a GcCell twice!");
        self.rooted.set(false);

        match self.cell.borrow_state() {
            BorrowState::Writing => (),
            _ => self.cell.borrow().unroot(),
        }
    }
}

/// A wrapper type for an immutably borrowed value from a `GcCell<T>`.
pub type GcCellRef<'a, T> = cell::Ref<'a, T>;

/// A wrapper type for a mutably borrowed value from a `GcCell<T>`.
pub struct GcCellRefMut<'a, T: Trace + ?Sized + 'static> {
    refm: cell::RefMut<'a, T>,
    rooted: &'a Cell<bool>,
}

impl<'a, T: Trace + ?Sized> Deref for GcCellRefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T { &*self.refm }
}

impl<'a, T: Trace + ?Sized> DerefMut for GcCellRefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T { &mut *self.refm }
}

impl<'a, T: Trace + ?Sized> Drop for GcCellRefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        // Restore the rooted state of the GcCell's contents to the state of the GcCell.
        // During the lifetime of the GcCellRefMut, the GcCell's contents are rooted.
        if !self.rooted.get() {
            unsafe { self.refm.unroot(); }
        }
    }
}

impl<T: Trace + Default> Default for GcCell<T> {
    #[inline]
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T: Trace + ?Sized + PartialEq> PartialEq for GcCell<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        *self.borrow() == *other.borrow()
    }

    #[inline(always)]
    fn ne(&self, other: &Self) -> bool {
        *self.borrow() != *other.borrow()
    }
}

impl<T: Trace + ?Sized + Eq> Eq for GcCell<T> {}

impl<T: Trace + ?Sized + PartialOrd> PartialOrd for GcCell<T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (*self.borrow()).partial_cmp(&*other.borrow())
    }

    #[inline(always)]
    fn lt(&self, other: &Self) -> bool {
        *self.borrow() < *other.borrow()
    }

    #[inline(always)]
    fn le(&self, other: &Self) -> bool {
        *self.borrow() <= *other.borrow()
    }

    #[inline(always)]
    fn gt(&self, other: &Self) -> bool {
        *self.borrow() > *other.borrow()
    }

    #[inline(always)]
    fn ge(&self, other: &Self) -> bool {
        *self.borrow() >= *other.borrow()
    }
}

impl<T: Trace + ?Sized + Ord> Ord for GcCell<T> {
    #[inline]
    fn cmp(&self, other: &GcCell<T>) -> Ordering {
        (*self.borrow()).cmp(&*other.borrow())
    }
}

impl<T: Trace + ?Sized + Hash> Hash for GcCell<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (*self.borrow()).hash(state);
    }
}

impl<T: Trace + ?Sized + fmt::Display> fmt::Display for GcCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.borrow(), f)
    }
}

impl<T: Trace + ?Sized + fmt::Debug> fmt::Debug for GcCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&*self.borrow(), f)
    }
}

impl<T: Trace> From<T> for GcCell<T> {
    fn from(t: T) -> Self {
        Self::new(t)
    }
}

