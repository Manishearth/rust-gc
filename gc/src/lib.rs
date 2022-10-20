//! Thread-local garbage-collected boxes (The `Gc<T>` type).
//!
//! The `Gc<T>` type provides shared ownership of an immutable value.
//! It is marked as non-sendable because the garbage collection only occurs
//! thread-locally.

#![cfg_attr(feature = "nightly", feature(coerce_unsized, unsize))]

use crate::gc::{GcBox, GcBoxHeader, GcBoxType};
use std::alloc::Layout;
use std::cell::{Cell, UnsafeCell};
use std::cmp::Ordering;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};
use std::rc::Rc;

#[cfg(feature = "nightly")]
use std::marker::Unsize;
#[cfg(feature = "nightly")]
use std::ops::CoerceUnsized;

mod gc;
#[cfg(feature = "serde")]
mod serde;
mod trace;
pub mod weak;

pub use weak::{WeakGc, WeakPair};

#[cfg(feature = "derive")]
pub use gc_derive::{Finalize, Trace};

/// `derive_prelude` is a quick prelude that imports
/// `Finalize`, `Trace`, and `GcPointer` for implementing
/// the derive
#[cfg(feature = "derive")]
pub mod derive_prelude {
    pub use crate::GcPointer;
    pub use gc_derive::{Finalize, Trace};
}

// We re-export the Trace method, as well as some useful internal methods for
// managing collections or configuring the garbage collector.
pub use crate::gc::{finalizer_safe, force_collect};
pub use crate::trace::{Finalize, Trace};

#[cfg(feature = "unstable-config")]
pub use crate::gc::{configure, GcConfig};
#[cfg(feature = "unstable-stats")]
pub use crate::gc::{stats, GcStats};

pub type GcPointer = NonNull<GcBox<dyn Trace>>;

////////
// Gc //
////////

/// A garbage-collected pointer type over an immutable value.
///
/// See the [module level documentation](./) for more details.
pub struct Gc<T: Trace + ?Sized + 'static> {
    ptr_root: Cell<NonNull<GcBox<T>>>,
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
            let ptr = GcBox::new(value, GcBoxType::Standard);

            // When we create a Gc<T>, all pointers which have been moved to the
            // heap no longer need to be rooted, so we unroot them.
            (*ptr.as_ptr()).value().unroot();
            let gc = Gc {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
                marker: PhantomData,
            };
            gc.set_root();
            gc
        }
    }
}

impl<T: Trace + ?Sized> Gc<T> {
    /// Returns `true` if the two `Gc`s point to the same allocation.
    pub fn ptr_eq(this: &Gc<T>, other: &Gc<T>) -> bool {
        GcBox::ptr_eq(this.inner(), other.inner())
    }
}

/// Returns the given pointer with its root bit cleared.
pub(crate) unsafe fn clear_root_bit<T: ?Sized + Trace>(
    ptr: NonNull<GcBox<T>>,
) -> NonNull<GcBox<T>> {
    let ptr = ptr.as_ptr();
    let data = ptr as *mut u8;
    let addr = data as isize;
    let ptr = set_data_ptr(ptr, data.wrapping_offset((addr & !1) - addr));
    NonNull::new_unchecked(ptr)
}

impl<T: Trace + ?Sized> Gc<T> {
    fn rooted(&self) -> bool {
        self.ptr_root.get().as_ptr() as *mut u8 as usize & 1 != 0
    }

    unsafe fn set_root(&self) {
        let ptr = self.ptr_root.get().as_ptr();
        let data = ptr as *mut u8;
        let addr = data as isize;
        let ptr = set_data_ptr(ptr, data.wrapping_offset((addr | 1) - addr));
        self.ptr_root.set(NonNull::new_unchecked(ptr));
    }

    unsafe fn clear_root(&self) {
        self.ptr_root.set(clear_root_bit(self.ptr_root.get()));
    }

    #[inline]
    fn inner_ptr(&self) -> *mut GcBox<T> {
        // If we are currently in the dropping phase of garbage collection,
        // it would be undefined behavior to dereference this pointer.
        // By opting into `Trace` you agree to not dereference this pointer
        // within your drop method, meaning that it should be safe.
        //
        // This assert exists just in case.
        assert!(finalizer_safe());

        unsafe { clear_root_bit(self.ptr_root.get()).as_ptr() }
    }

    #[inline]
    fn inner(&self) -> &GcBox<T> {
        unsafe { &*self.inner_ptr() }
    }
}

impl<T: Trace + ?Sized> Gc<T> {
    /// Consumes the `Gc`, returning the wrapped pointer.
    ///
    /// To avoid a memory leak, the pointer must be converted back into a `Gc`
    /// using [`Gc::from_raw`][from_raw].
    ///
    /// [from_raw]: struct.Gc.html#method.from_raw
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::Gc;
    ///
    /// let x = Gc::new(22);
    /// let x_ptr = Gc::into_raw(x);
    /// assert_eq!(unsafe { *x_ptr }, 22);
    /// ```
    pub fn into_raw(this: Self) -> *const T {
        let ptr: *const T = GcBox::value_ptr(this.inner_ptr());
        mem::forget(this);
        ptr
    }

    /// Constructs an `Gc` from a raw pointer.
    ///
    /// The raw pointer must have been previously returned by a call to a
    /// [`Gc::into_raw`][into_raw].
    ///
    /// This function is unsafe because improper use may lead to memory
    /// problems. For example, a use-after-free will occur if the function is
    /// called twice on the same raw pointer.
    ///
    /// [into_raw]: struct.Gc.html#method.into_raw
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::Gc;
    ///
    /// let x = Gc::new(22);
    /// let x_ptr = Gc::into_raw(x);
    ///
    /// unsafe {
    ///     // Convert back to an `Gc` to prevent leak.
    ///     let x = Gc::from_raw(x_ptr);
    ///     assert_eq!(*x, 22);
    ///
    ///     // Further calls to `Gc::from_raw(x_ptr)` would be memory unsafe.
    /// }
    ///
    /// // The memory can be freed at any time after `x` went out of scope above
    /// // (when the collector is run), which would result in `x_ptr` dangling!
    /// ```
    pub unsafe fn from_raw(ptr: *const T) -> Self {
        // Find the offset of T in GcBox<T>. Note that Layout::extend
        // relies on GcBox being repr(C).
        let (_, offset) = Layout::new::<GcBoxHeader>()
            .extend(Layout::for_value::<T>(&*ptr))
            .unwrap();

        // Reverse the offset to find the original GcBox.
        let fake_ptr = ptr as *mut GcBox<T>;
        let rc_ptr = set_data_ptr(fake_ptr, (ptr as *mut u8).offset(-(offset as isize)));

        let gc = Gc {
            ptr_root: Cell::new(NonNull::new_unchecked(rc_ptr)),
            marker: PhantomData,
        };
        gc.set_root();
        gc
    }

    #[inline]
    pub fn clone_weak_gc(&self) -> WeakGc<T> {
        unsafe {
            let weak_gc = WeakGc::from_gc_box(self.ptr_root.get());
            weak_gc
        }
    }

    #[inline]
    pub fn create_weak_pair<V>(&self, value: Option<V>) -> WeakPair<T, V>
    where
        V: Trace,
    {
        let weak_pair = WeakPair::from_gc_value_pair(self.ptr_root.get(), value);
        weak_pair
    }
}

impl<T: Trace + ?Sized> Finalize for Gc<T> {}

unsafe impl<T: Trace + ?Sized> Trace for Gc<T> {
    #[inline]
    unsafe fn trace(&self) {
        self.inner().trace_inner();
    }

    #[inline]
    unsafe fn is_marked_ephemeron(&self) -> bool {
        false
    }

    #[inline]
    unsafe fn weak_trace(&self, queue: &mut Vec<GcPointer>) {
        self.inner().weak_trace_inner(queue);
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized + Debug> Debug for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized> fmt::Pointer for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.inner(), f)
    }
}

impl<T: Trace> From<T> for Gc<T> {
    fn from(t: T) -> Self {
        Self::new(t)
    }
}

impl<T: Trace + ?Sized> std::borrow::Borrow<T> for Gc<T> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: Trace + ?Sized> std::convert::AsRef<T> for Gc<T> {
    fn as_ref(&self) -> &T {
        &**self
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
    pub fn borrow(&self) -> GcCellRef<'_, T> {
        match self.try_borrow() {
            Ok(value) => value,
            Err(e) => panic!("{}", e),
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
    pub fn borrow_mut(&self) -> GcCellRefMut<'_, T> {
        match self.try_borrow_mut() {
            Ok(value) => value,
            Err(e) => panic!("{}", e),
        }
    }

    /// Immutably borrows the wrapped value, returning an error if the value is currently mutably
    /// borrowed.
    ///
    /// The borrow lasts until the returned `GcCellRef` exits scope. Multiple immutable borrows can be
    /// taken out at the same time.
    ///
    /// This is the non-panicking variant of [`borrow`](#method.borrow).
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::GcCell;
    ///
    /// let c = GcCell::new(5);
    ///
    /// {
    ///     let m = c.borrow_mut();
    ///     assert!(c.try_borrow().is_err());
    /// }
    ///
    /// {
    ///     let m = c.borrow();
    ///     assert!(c.try_borrow().is_ok());
    /// }
    /// ```
    pub fn try_borrow(&self) -> Result<GcCellRef<'_, T>, BorrowError> {
        if self.flags.get().borrowed() == BorrowState::Writing {
            return Err(BorrowError);
        }
        self.flags.set(self.flags.get().add_reading());

        // This will fail if the borrow count overflows, which shouldn't happen,
        // but let's be safe
        assert!(self.flags.get().borrowed() == BorrowState::Reading);

        unsafe {
            Ok(GcCellRef {
                flags: &self.flags,
                value: &*self.cell.get(),
            })
        }
    }

    /// Mutably borrows the wrapped value, returning an error if the value is currently borrowed.
    ///
    /// The borrow lasts until the returned `GcCellRefMut` exits scope.
    /// The value cannot be borrowed while this borrow is active.
    ///
    /// This is the non-panicking variant of [`borrow_mut`](#method.borrow_mut).
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::GcCell;
    ///
    /// let c = GcCell::new(5);
    ///
    /// {
    ///     let m = c.borrow();
    ///     assert!(c.try_borrow_mut().is_err());
    /// }
    ///
    /// assert!(c.try_borrow_mut().is_ok());
    /// ```
    pub fn try_borrow_mut(&self) -> Result<GcCellRefMut<'_, T>, BorrowMutError> {
        if self.flags.get().borrowed() != BorrowState::Unused {
            return Err(BorrowMutError);
        }
        self.flags.set(self.flags.get().set_writing());

        unsafe {
            // Force the val_ref's contents to be rooted for the duration of the
            // mutable borrow
            if !self.flags.get().rooted() {
                (*self.cell.get()).root();
            }

            Ok(GcCellRefMut {
                gc_cell: self,
                value: &mut *self.cell.get(),
            })
        }
    }
}

/// An error returned by [`GcCell::try_borrow`](struct.GcCell.html#method.try_borrow).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default, Hash)]
pub struct BorrowError;

impl std::fmt::Display for BorrowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt("GcCell<T> already mutably borrowed", f)
    }
}

/// An error returned by [`GcCell::try_borrow_mut`](struct.GcCell.html#method.try_borrow_mut).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default, Hash)]
pub struct BorrowMutError;

impl std::fmt::Display for BorrowMutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt("GcCell<T> already borrowed", f)
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
    unsafe fn is_marked_ephemeron(&self) -> bool {
        false
    }

    #[inline]
    unsafe fn weak_trace(&self, queue: &mut Vec<GcPointer>) {
        match self.flags.get().borrowed() {
            BorrowState::Writing => (),
            _ => (*self.cell.get()).weak_trace(queue),
        }
    }

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
pub struct GcCellRef<'a, T: ?Sized + 'static> {
    flags: &'a Cell<BorrowFlag>,
    value: &'a T,
}

impl<'a, T: ?Sized> GcCellRef<'a, T> {
    /// Copies a `GcCellRef`.
    ///
    /// The `GcCell` is already immutably borrowed, so this cannot fail.
    ///
    /// This is an associated function that needs to be used as
    /// `GcCellRef::clone(...)`. A `Clone` implementation or a method
    /// would interfere with the use of `c.borrow().clone()` to clone
    /// the contents of a `GcCell`.
    #[inline]
    pub fn clone(orig: &GcCellRef<'a, T>) -> GcCellRef<'a, T> {
        orig.flags.set(orig.flags.get().add_reading());
        GcCellRef {
            flags: orig.flags,
            value: orig.value,
        }
    }

    /// Makes a new `GcCellRef` from a component of the borrowed data.
    ///
    /// The `GcCell` is already immutably borrowed, so this cannot fail.
    ///
    /// This is an associated function that needs to be used as `GcCellRef::map(...)`.
    /// A method would interfere with methods of the same name on the contents
    /// of a `GcCellRef` used through `Deref`.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::{GcCell, GcCellRef};
    ///
    /// let c = GcCell::new((5, 'b'));
    /// let b1: GcCellRef<(u32, char)> = c.borrow();
    /// let b2: GcCellRef<u32> = GcCellRef::map(b1, |t| &t.0);
    /// //assert_eq!(b2, 5);
    /// ```
    #[inline]
    pub fn map<U, F>(orig: Self, f: F) -> GcCellRef<'a, U>
    where
        U: ?Sized,
        F: FnOnce(&T) -> &U,
    {
        let ret = GcCellRef {
            flags: orig.flags,
            value: f(orig.value),
        };

        // We have to tell the compiler not to call the destructor of GcCellRef,
        // because it will update the borrow flags.
        std::mem::forget(orig);

        ret
    }

    /// Splits a `GcCellRef` into multiple `GcCellRef`s for different components of the borrowed data.
    ///
    /// The `GcCell` is already immutably borrowed, so this cannot fail.
    ///
    /// This is an associated function that needs to be used as GcCellRef::map_split(...).
    /// A method would interfere with methods of the same name on the contents of a `GcCellRef` used through `Deref`.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::{GcCell, GcCellRef};
    ///
    /// let cell = GcCell::new((1, 'c'));
    /// let borrow = cell.borrow();
    /// let (first, second) = GcCellRef::map_split(borrow, |x| (&x.0, &x.1));
    /// assert_eq!(*first, 1);
    /// assert_eq!(*second, 'c');
    /// ```
    #[inline]
    pub fn map_split<U, V, F>(orig: Self, f: F) -> (GcCellRef<'a, U>, GcCellRef<'a, V>)
    where
        U: ?Sized,
        V: ?Sized,
        F: FnOnce(&T) -> (&U, &V),
    {
        let (a, b) = f(orig.value);

        orig.flags.set(orig.flags.get().add_reading());

        let ret = (
            GcCellRef {
                flags: orig.flags,
                value: a,
            },
            GcCellRef {
                flags: orig.flags,
                value: b,
            },
        );

        // We have to tell the compiler not to call the destructor of GcCellRef,
        // because it will update the borrow flags.
        std::mem::forget(orig);

        ret
    }
}

impl<'a, T: ?Sized> Deref for GcCellRef<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'a, T: ?Sized> Drop for GcCellRef<'a, T> {
    fn drop(&mut self) {
        debug_assert!(self.flags.get().borrowed() == BorrowState::Reading);
        self.flags.set(self.flags.get().sub_reading());
    }
}

impl<'a, T: ?Sized + Debug> Debug for GcCellRef<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<'a, T: ?Sized + Display> Display for GcCellRef<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

/// A wrapper type for a mutably borrowed value from a `GcCell<T>`.
pub struct GcCellRefMut<'a, T: Trace + ?Sized + 'static, U: ?Sized = T> {
    gc_cell: &'a GcCell<T>,
    value: &'a mut U,
}

impl<'a, T: Trace + ?Sized, U: ?Sized> GcCellRefMut<'a, T, U> {
    /// Makes a new `GcCellRefMut` for a component of the borrowed data, e.g., an enum
    /// variant.
    ///
    /// The `GcCellRefMut` is already mutably borrowed, so this cannot fail.
    ///
    /// This is an associated function that needs to be used as
    /// `GcCellRefMut::map(...)`. A method would interfere with methods of the same
    /// name on the contents of a `GcCell` used through `Deref`.
    ///
    /// # Examples
    ///
    /// ```
    /// use gc::{GcCell, GcCellRefMut};
    ///
    /// let c = GcCell::new((5, 'b'));
    /// {
    ///     let b1: GcCellRefMut<(u32, char)> = c.borrow_mut();
    ///     let mut b2: GcCellRefMut<(u32, char), u32> = GcCellRefMut::map(b1, |t| &mut t.0);
    ///     assert_eq!(*b2, 5);
    ///     *b2 = 42;
    /// }
    /// assert_eq!(*c.borrow(), (42, 'b'));
    /// ```
    #[inline]
    pub fn map<V, F>(orig: Self, f: F) -> GcCellRefMut<'a, T, V>
    where
        V: ?Sized,
        F: FnOnce(&mut U) -> &mut V,
    {
        let value = unsafe { &mut *(orig.value as *mut U) };

        let ret = GcCellRefMut {
            gc_cell: orig.gc_cell,
            value: f(value),
        };

        // We have to tell the compiler not to call the destructor of GcCellRefMut,
        // because it will update the borrow flags.
        std::mem::forget(orig);

        ret
    }
}

impl<'a, T: Trace + ?Sized, U: ?Sized> Deref for GcCellRefMut<'a, T, U> {
    type Target = U;

    #[inline]
    fn deref(&self) -> &U {
        self.value
    }
}

impl<'a, T: Trace + ?Sized, U: ?Sized> DerefMut for GcCellRefMut<'a, T, U> {
    #[inline]
    fn deref_mut(&mut self) -> &mut U {
        self.value
    }
}

impl<'a, T: Trace + ?Sized, U: ?Sized> Drop for GcCellRefMut<'a, T, U> {
    #[inline]
    fn drop(&mut self) {
        debug_assert!(self.gc_cell.flags.get().borrowed() == BorrowState::Writing);
        // Restore the rooted state of the GcCell's contents to the state of the GcCell.
        // During the lifetime of the GcCellRefMut, the GcCell's contents are rooted.
        if !self.gc_cell.flags.get().rooted() {
            unsafe {
                (*self.gc_cell.cell.get()).unroot();
            }
        }
        self.gc_cell
            .flags
            .set(self.gc_cell.flags.get().set_unused());
    }
}

impl<'a, T: Trace + ?Sized, U: Debug + ?Sized> Debug for GcCellRefMut<'a, T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&*(self.deref()), f)
    }
}

impl<'a, T: Trace + ?Sized, U: Display + ?Sized> Display for GcCellRefMut<'a, T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.flags.get().borrowed() {
            BorrowState::Unused | BorrowState::Reading => f
                .debug_struct("GcCell")
                .field("value", &self.borrow())
                .finish(),
            BorrowState::Writing => f
                .debug_struct("GcCell")
                .field("value", &"<borrowed>")
                .finish(),
        }
    }
}

// Sets the data pointer of a `?Sized` raw pointer.
//
// For a slice/trait object, this sets the `data` field and leaves the rest
// unchanged. For a sized raw pointer, this simply sets the pointer.
pub(crate) unsafe fn set_data_ptr<T: ?Sized, U>(mut ptr: *mut T, data: *mut U) -> *mut T {
    ptr::write(&mut ptr as *mut _ as *mut *mut u8, data as *mut u8);
    ptr
}
