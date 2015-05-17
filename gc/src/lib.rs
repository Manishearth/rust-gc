#![feature(std_misc, optin_builtin_traits)]

// XXXManishearth see #1
#![allow(unused_unsafe)]

use std::cell::{self, Cell, RefCell, BorrowState};
use std::ops::{Deref, DerefMut};
use std::marker;
use gc::GcBox;

mod gc;
pub mod trace;

#[cfg(test)]
mod test;

pub use trace::Trace;
pub use gc::{force_collect, GcBoxTrait};

////////
// Gc //
////////

pub struct Gc<T: Trace + 'static> {
    // XXX We should see if we can use the highest byte of _ptr
    // for this flag. Unfortunately this flag is necessary so that
    // we can unroot when we are dropped.
    root: Cell<bool>,
    _ptr: *mut GcBox<T>,
}

impl<T> !marker::Send for Gc<T> {}

impl<T> !marker::Sync for Gc<T> {}

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

    fn inner(&self) -> &GcBox<T> {
        unsafe { &*self._ptr }
    }
}

impl<T: Trace> Trace for Gc<T> {
    fn trace(&self) {
        self.inner().trace_inner();
    }

    fn root(&self) {
        assert!(!self.root.get(), "Can't double-root a Gc<T>");
        self.root.set(true);
        unsafe {
            // This unsafe block is wrong! (see #1)
            self.inner().root_inner();
        }
    }

    fn unroot(&self) {
        assert!(self.root.get(), "Can't double-unroot a Gc<T>");
        self.root.set(false);
        unsafe {
            // This unsafe block is wrong! (see #1)
            self.inner().unroot_inner();
        }
    }
}

impl<T: Trace> Clone for Gc<T> {
    fn clone(&self) -> Gc<T> {
        self.root();
        Gc { _ptr: self._ptr, root: Cell::new(true) }
    }
}

impl<T: Trace> Deref for Gc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner().value()
    }
}

impl<T: Trace> Drop for Gc<T> {
    fn drop(&mut self) {
        // This will be safe, because if we are a root, we cannot
        // be being collected by the garbage collector, and thus
        // our reference is guaranteed to be valid.
        //
        // If we are being collected by the garbage collector (we are
        // not a root), then our reference may not be valid anymore due to cycles
        if self.root.get() {
            self.unroot();
        }
    }
}

////////////
// GcCell //
////////////

/// A mutable garbage collected pointer/cell hybrid
pub struct GcCell<T: 'static> {
    cell: RefCell<T>,
}

impl<T> !marker::Send for GcCell<T> {}

impl<T> !marker::Sync for GcCell<T> {}

impl <T: Trace> GcCell<T> {
    pub fn new(value: T) -> GcCell<T> {
        GcCell{
            cell: RefCell::new(value)
        }
    }

    pub fn borrow(&self) -> GcCellRef<T> {
        self.cell.borrow()
    }

    pub fn borrow_mut(&self) -> GcCellRefMut<T> {
        let val_ref = self.cell.borrow_mut();

        // Root everything inside the box for the lifetime of the GcCellRefMut
        val_ref.root();

        GcCellRefMut {
            _ref: val_ref,
        }
    }
}

impl<T: Trace> Trace for GcCell<T> {
    fn trace(&self) {
        match self.cell.borrow_state() {
            // We don't go in, because it would panic!(),
            // and also everything inside is already rooted
            BorrowState::Writing => (),
            _ => self.cell.borrow().trace(),
        }
    }

    fn root(&self) {
        // XXX: Maybe handle this better than panicking? (can we just dodge the check?)
        self.cell.borrow().root();
    }

    fn unroot(&self) {
        // XXX: Maybe handle this better than panicking? (can we just dodge the check?)
        self.cell.borrow().unroot();
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
pub struct GcCellRefMut<'a, T: Trace + 'static> {
    _ref: ::std::cell::RefMut<'a, T>,
}

impl<'a, T: Trace> Deref for GcCellRefMut<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T { &*self._ref }
}

impl<'a, T: Trace> DerefMut for GcCellRefMut<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T { &mut *self._ref }
}

impl<'a, T: Trace> Drop for GcCellRefMut<'a, T> {
    fn drop(&mut self) {
        // the data is now within a gc tree again
        // we don't have to keep it alive explicitly any longer
        self._ref.unroot();
    }
}
