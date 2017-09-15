//! Thread-local garbage-collected boxes (The `Gc<T>` type).
//!
//! The `Gc<T>` type provides shared ownership of an immutable value.
//! It is marked as non-sendable because the garbage collection only occurs
//! thread-locally.

#![cfg_attr(feature = "nightly",
            feature(coerce_unsized,
                    nonzero,
                    optin_builtin_traits,
                    shared,
                    unsize,
                    specialization))]

use gc::GcBox;
use std::cell::{Cell, UnsafeCell};
use std::cmp::Ordering;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

#[cfg(feature = "nightly")]
use std::marker::Unsize;
#[cfg(feature = "nightly")]
use std::ops::CoerceUnsized;
#[cfg(feature = "nightly")]
use std::ptr::Shared;

#[cfg(not(feature = "nightly"))]
mod stable;
#[cfg(not(feature = "nightly"))]
use stable::Shared;

mod gc;
mod trace;

// We re-export the Trace method, as well as some useful internal methods for
// managing collections or configuring the garbage collector.
pub use trace::{Finalize, Trace};
pub use gc::{force_collect, finalizer_safe};

////////
// Gc //
////////

/// A garbage-collected pointer type over an immutable value.
///
/// See the [module level documentation](./) for more details.
pub struct Gc<T: Trace + ?Sized + 'static> {
    ptr_root: Cell<Shared<GcBox<T>>>,
    marker: PhantomData<Rc<T>>,
}

#[cfg(feature = "nightly")]
impl<T: Trace + ?Sized + Unsize<U>, U: Trace + ?Sized> CoerceUnsized<Gc<U>> for Gc<T> {}

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
    /// assert_eq!(*five, 5);
    /// ```
    pub fn new(value: T) -> Self {
        assert!(mem::align_of::<GcBox<T>>() > 1);

        unsafe {
            // Allocate the memory for the object
            let ptr = GcBox::new(value);

            // When we create a Gc<T>, all pointers which have been moved to the
            // heap no longer need to be rooted, so we unroot them.
            (*ptr.as_ptr()).value().unroot();
            let gc = Gc {
                ptr_root: Cell::new(Shared::new_unchecked(ptr.as_ptr())),
                marker: PhantomData,
            };
            gc.set_root();
            gc
        }
    }
}

/// Returns the given pointer with its root bit cleared.
unsafe fn clear_root_bit<T: ?Sized + Trace>(ptr: Shared<GcBox<T>>) -> Shared<GcBox<T>> {
    let mut ptr = ptr.as_ptr();
    *(&mut ptr as *mut _ as *mut usize) &= !1;
    // *(&mut ptr as *mut *const GcBox<T> as *mut usize) &= !1;
    Shared::new_unchecked(ptr)
}

impl<T: Trace + ?Sized> Gc<T> {
    fn rooted(&self) -> bool {
        self.ptr_root.get().as_ptr() as *mut u8 as usize & 1 != 0
    }

    unsafe fn set_root(&self) {
        let mut ptr = self.ptr_root.get().as_ptr();
        *(&mut ptr as *mut *mut GcBox<T> as *mut usize) |= 1;
        self.ptr_root.set(Shared::new_unchecked(ptr));
    }

    unsafe fn clear_root(&self) {
        self.ptr_root.set(clear_root_bit(self.ptr_root.get()));
    }

    #[inline]
    fn inner(&self) -> &GcBox<T> {
        // If we are currently in the dropping phase of garbage collection,
        // it would be undefined behavior to dereference this pointer.
        // By opting into `Trace` you agree to not dereference this pointer
        // within your drop method, meaning that it should be safe.
        //
        // This assert exists just in case.
        assert!(finalizer_safe());

        unsafe { &*clear_root_bit(self.ptr_root.get()).as_ptr() }
    }
}

impl<T: Trace + ?Sized> Finalize for Gc<T> {}

unsafe impl<T: Trace + ?Sized> Trace for Gc<T> {
    #[inline]
    unsafe fn trace(&self) {
        self.inner().trace_inner();
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.rooted(), "Can't double-root a Gc<T>");

        // Try to get inner before modifying our state. Inner may be
        // inaccessible due to this method being invoked during the sweeping
        // phase, and we don't want to modify our state before panicking.
        self.inner().root_inner();

        self.set_root();
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.rooted(), "Can't double-unroot a Gc<T>");

        // Try to get inner before modifying our state. Inner may be
        // inaccessible due to this method being invoked during the sweeping
        // phase, and we don't want to modify our state before panicking.
        self.inner().unroot_inner();

        self.clear_root();
    }

    #[inline]
    fn finalize_glue(&self) {
        Finalize::finalize(self);
    }
}

impl<T: Trace + ?Sized> Clone for Gc<T> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            self.inner().root_inner();
            let gc = Gc {
                ptr_root: Cell::new(self.ptr_root.get()),
                marker: PhantomData,
            };
            gc.set_root();
            gc
        }
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
        if self.rooted() {
            unsafe {
                self.inner().unroot_inner();
            }
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

impl<T: Trace + ?Sized + Display> Display for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized + Debug> Debug for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<T: Trace> fmt::Pointer for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.inner(), f)
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

/// The BorrowFlag used by GC is split into 2 parts. the upper 63 or 31 bits
/// (depending on the architecture) are used to store the number of borrowed
/// references to the type. The low bit is used to record the rootedness of the
/// type.
///
/// This means that GcCell can have, at maximum, half as many outstanding
/// borrows as RefCell before panicking. I don't think that will be a problem.
#[derive(Copy, Clone)]
struct BorrowFlag(usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BorrowState {
    Reading,
    Writing,
    Unused,
}

const ROOT: usize = 1;
const WRITING: usize = !1;
const UNUSED: usize = 0;

/// The base borrowflag init is rooted, and has no outstanding borrows.
const BORROWFLAG_INIT: BorrowFlag = BorrowFlag(1);

impl BorrowFlag {
    fn borrowed(self) -> BorrowState {
        match self.0 & !ROOT {
            UNUSED => BorrowState::Unused,
            WRITING => BorrowState::Writing,
            _ => BorrowState::Reading,
        }
    }

    fn rooted(self) -> bool {
        match self.0 & ROOT {
            0 => false,
            _ => true,
        }
    }

    fn set_writing(self) -> Self {
        // Set every bit other than the root bit, which is preserved
        BorrowFlag(self.0 | WRITING)
    }

    fn set_unused(self) -> Self {
        // Clear every bit other than the root bit, which is preserved
        BorrowFlag(self.0 & ROOT)
    }

    fn add_reading(self) -> Self {
        assert!(self.borrowed() != BorrowState::Writing);
        // Add 1 to the integer starting at the second binary digit. As our
        // borrowstate is not writing, we know that overflow cannot happen, so
        // this is equivalent to the following, more complicated, expression:
        //
        // BorrowFlag((self.0 & ROOT) | (((self.0 >> 1) + 1) << 1))
        BorrowFlag(self.0 + 0b10)
    }

    fn sub_reading(self) -> Self {
        assert!(self.borrowed() == BorrowState::Reading);
        // Subtract 1 from the integer starting at the second binary digit. As
        // our borrowstate is not writing or unused, we know that overflow or
        // undeflow cannot happen, so this is equivalent to the following, more
        // complicated, expression:
        //
        // BorrowFlag((self.0 & ROOT) | (((self.0 >> 1) - 1) << 1))
        BorrowFlag(self.0 - 0b10)
    }

    fn set_rooted(self, rooted: bool) -> Self {
        // Preserve the non-root bits
        BorrowFlag((self.0 & !ROOT) | (rooted as usize))
    }
}


/// A mutable memory location with dynamically checked borrow rules
/// that can be used inside of a garbage-collected pointer.
///
/// This object is a `RefCell` that can be used inside of a `Gc<T>`.
pub struct GcCell<T: ?Sized + 'static> {
    flags: Cell<BorrowFlag>,
    cell: UnsafeCell<T>,
}

impl<T: Trace> GcCell<T> {
    /// Creates a new `GcCell` containing `value`.
    #[inline]
    pub fn new(value: T) -> Self {
        GcCell {
            flags: Cell::new(BORROWFLAG_INIT),
            cell: UnsafeCell::new(value),
        }
    }

    /// Consumes the `GcCell`, returning the wrapped value.
    #[inline]
    pub fn into_inner(self) -> T {
        unsafe { self.cell.into_inner() }
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
        if self.flags.get().borrowed() == BorrowState::Writing {
            panic!("GcCell<T> already mutably borrowed");
        }
        self.flags.set(self.flags.get().add_reading());

        // This will fail if the borrow count overflows, which shouldn't happen,
        // but let's be safe
        assert!(self.flags.get().borrowed() == BorrowState::Reading);

        unsafe {
            GcCellRef {
                flags: &self.flags,
                value: &*self.cell.get(),
            }
        }
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
        if self.flags.get().borrowed() != BorrowState::Unused {
            panic!("GcCell<T> already borrowed");
        }
        self.flags.set(self.flags.get().set_writing());

        unsafe {
            // Force the val_ref's contents to be rooted for the duration of the
            // mutable borrow
            if !self.flags.get().rooted() {
                (*self.cell.get()).root();
            }

            GcCellRefMut {
                flags: &self.flags,
                value: &mut *self.cell.get(),
            }
        }
    }
}

impl<T: Trace + ?Sized> Finalize for GcCell<T> {}

unsafe impl<T: Trace + ?Sized> Trace for GcCell<T> {
    #[inline]
    unsafe fn trace(&self) {
        match self.flags.get().borrowed() {
            BorrowState::Writing => (),
            _ => (*self.cell.get()).trace(),
        }
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.flags.get().rooted(), "Can't root a GcCell twice!");
        self.flags.set(self.flags.get().set_rooted(true));

        match self.flags.get().borrowed() {
            BorrowState::Writing => (),
            _ => (*self.cell.get()).root(),
        }
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.flags.get().rooted(), "Can't unroot a GcCell twice!");
        self.flags.set(self.flags.get().set_rooted(false));

        match self.flags.get().borrowed() {
            BorrowState::Writing => (),
            _ => (*self.cell.get()).unroot(),
        }
    }

    #[inline]
    fn finalize_glue(&self) {
        Finalize::finalize(self);
        match self.flags.get().borrowed() {
            BorrowState::Writing => (),
            _ => unsafe { (*self.cell.get()).finalize_glue() },
        }
    }
}


/// A wrapper type for an immutably borrowed value from a `GcCell<T>`.
pub struct GcCellRef<'a, T: Trace + ?Sized + 'static> {
    flags: &'a Cell<BorrowFlag>,
    value: &'a T,
}

impl<'a, T: Trace + ?Sized> Deref for GcCellRef<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T: Trace + ?Sized> Drop for GcCellRef<'a, T> {
    fn drop(&mut self) {
        debug_assert!(self.flags.get().borrowed() == BorrowState::Reading);
        self.flags.set(self.flags.get().sub_reading());
    }
}

impl<'a, T: Trace + ?Sized + Debug> Debug for GcCellRef<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}


/// A wrapper type for a mutably borrowed value from a `GcCell<T>`.
pub struct GcCellRefMut<'a, T: Trace + ?Sized + 'static> {
    flags: &'a Cell<BorrowFlag>,
    value: &'a mut T,
}

impl<'a, T: Trace + ?Sized> Deref for GcCellRefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T: Trace + ?Sized> DerefMut for GcCellRefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}

impl<'a, T: Trace + ?Sized> Drop for GcCellRefMut<'a, T> {
    #[inline]
    fn drop(&mut self) {
        debug_assert!(self.flags.get().borrowed() == BorrowState::Writing);
        // Restore the rooted state of the GcCell's contents to the state of the GcCell.
        // During the lifetime of the GcCellRefMut, the GcCell's contents are rooted.
        if !self.flags.get().rooted() {
            unsafe {
                self.value.unroot();
            }
        }
        self.flags.set(self.flags.get().set_unused());
    }
}

impl<'a, T: Trace + ?Sized + Debug> Debug for GcCellRefMut<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&*(self.deref()), f)
    }
}


unsafe impl<T: ?Sized + Send> Send for GcCell<T> {}

impl<T: Trace + Clone> Clone for GcCell<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new(self.borrow().clone())
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

impl<T: Trace + ?Sized + Debug> Debug for GcCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.flags.get().borrowed() {
            BorrowState::Unused | BorrowState::Reading => {
                f.debug_struct("GcCell")
                    .field("value", &self.borrow())
                    .finish()
            }
            BorrowState::Writing => {
                f.debug_struct("GcCell")
                    .field("value", &"<borrowed>")
                    .finish()
            }
        }
    }
}
