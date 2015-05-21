#![feature(std_misc, optin_builtin_traits, core)]

use std::cell::{self, Cell, RefCell, BorrowState};
use std::ops::{Deref, DerefMut, CoerceUnsized};
use std::marker;
use gc::{GcBox, GcBoxTrait};

mod gc;
mod trace;

pub use trace::Trace;
pub use gc::force_collect;

////////
// Gc //
////////

pub struct Gc<T: Trace + ?Sized + 'static> {
    // XXX We should see if we can use the highest byte of _ptr
    // for this flag. Unfortunately this flag is necessary so that
    // we can unroot when we are dropped.
    root: Cell<bool>,
    _ptr: *mut GcBox<T>,
}

impl<T: ?Sized> !marker::Send for Gc<T> {}

impl<T: ?Sized> !marker::Sync for Gc<T> {}

impl<T: Trace + ?Sized + marker::Unsize<U>, U: Trace + ?Sized> CoerceUnsized<Gc<U>> for Gc<T> {}

impl<T: Trace> Gc<T> {
    pub fn new(value: T) -> Gc<T> {
        unsafe {
            // Allocate the box first
            let ptr = GcBox::new(value);

            // The thing which we are storing internally is no longer rooted!
            (*ptr).value().unroot();
            Gc { _ptr: ptr, root: Cell::new(true) }
        }
    }
}

impl<T: Trace + ?Sized> Gc<T> {
    fn inner(&self) -> &GcBox<T> {
        unsafe { &*self._ptr }
    }
}

impl<T: Trace + ?Sized> Trace for Gc<T> {
    unsafe fn trace(&self) {
        self.inner().trace_inner();
    }

    unsafe fn root(&self) {
        assert!(!self.root.get(), "Can't double-root a Gc<T>");
        self.root.set(true);
        self.inner().root_inner();
    }

    unsafe fn unroot(&self) {
        assert!(self.root.get(), "Can't double-unroot a Gc<T>");
        self.root.set(false);
        self.inner().unroot_inner();
    }
}

impl<T: Trace + ?Sized> Clone for Gc<T> {
    fn clone(&self) -> Gc<T> {
        unsafe { self.inner().root_inner(); }
        Gc { _ptr: self._ptr, root: Cell::new(true) }
    }
}

impl<T: Trace + ?Sized> Deref for Gc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner().value()
    }
}

impl<T: Trace + ?Sized> Drop for Gc<T> {
    fn drop(&mut self) {
        // This will be safe, because if we are a root, we cannot
        // be being collected by the garbage collector, and thus
        // our reference is guaranteed to be valid.
        //
        // If we are being collected by the garbage collector (we are
        // not a root), then our reference may not be valid anymore due to cycles
        if self.root.get() {
            unsafe { self.unroot(); }
        }
    }
}

////////////
// GcCell //
////////////

/// A mutable garbage collected pointer/cell hybrid
pub struct GcCell<T: ?Sized + 'static> {
    rooted: Cell<bool>,
    cell: RefCell<T>,
}

impl <T: Trace> GcCell<T> {
    pub fn new(value: T) -> GcCell<T> {
        GcCell{
            rooted: Cell::new(true),
            cell: RefCell::new(value),
        }
    }
}

impl <T: Trace + ?Sized> GcCell<T> {
    pub fn borrow(&self) -> GcCellRef<T> {
        self.cell.borrow()
    }

    pub fn borrow_mut(&self) -> GcCellRefMut<T> {
        let val_ref = self.cell.borrow_mut();

        if !self.rooted.get() {
            // Root everything inside the box for the lifetime of the GcCellRefMut
            unsafe { val_ref.root(); }
        }

        GcCellRefMut {
            _ref: val_ref,
            _rooted: &self.rooted,
        }
    }
}

impl<T: Trace + ?Sized> Trace for GcCell<T> {
    unsafe fn trace(&self) {
        match self.cell.borrow_state() {
            // We don't go in, because it would panic!(),
            // and also everything inside is already rooted
            BorrowState::Writing => (),
            _ => self.cell.borrow().trace(),
        }
    }

    unsafe fn root(&self) {
        assert!(!self.rooted.get(), "Can't root a GcCell Twice!");
        self.rooted.set(true);
        match self.cell.borrow_state() {
            // We don't go in, because it would panic!(),
            // and also everything inside is already rooted
            BorrowState::Writing => (),
            _ => self.cell.borrow().root(),
        }
    }

    unsafe fn unroot(&self) {
        assert!(self.rooted.get(), "Can't unroot a GcCell Twice!");
        self.rooted.set(false);
        match self.cell.borrow_state() {
            // We don't go in, because it would panic!(),
            // and also everything inside is rooted, and will
            // be unrooted automatically because of the above .set()
            BorrowState::Writing => (),
            _ => self.cell.borrow().unroot(),
        }
    }
}

/// In the non-mutable case, nothing interesting needs to happen. We are just taking
/// a reference into the RefCell.
pub type GcCellRef<'a, T> = cell::Ref<'a, T>;

/// The GcCellRefMut struct acts as a RAII guard (like RefCell's RefMut), which
/// will provides unique mutable access to the value inside the GcCell while also
/// ensuring that the data isn't collected incorrectly
/// This means that it roots the internal box (to ensure that the pointer remains alive),
/// (although this is probably unnecessary - investigate).
/// as well as the data inside the box. (as it may be modified, or moved out of the object)
/// When this guard is present, it is not possible for the trace implementation to see inside
/// the object, so the object inside must be rooted to prevent it from being collected.
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
    fn drop(&mut self) {
        if !self._rooted.get() {
            // the data is now within a gc tree again
            // we don't have to keep it alive explicitly any longer
            unsafe { self._ref.unroot(); }
        }
    }
}
