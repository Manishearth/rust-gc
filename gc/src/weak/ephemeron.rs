//! This module will implement the internal types GcBox and Ephemeron
use crate::gc::{finalizer_safe, GcBox, GcBoxType};
use crate::trace::Trace;
use crate::{clear_root_bit, set_data_ptr, Finalize, GcPointer};
use std::cell::Cell;
use std::mem;
use std::ptr::NonNull;

/// Implementation of an Ephemeron structure
///
/// An Ephemeron can be either a WeakPair (Ephemeron<K,V>) or a WeakBox (Ephemeron<K,()>)
///
///
/// # Tracing with Ephemerons
///
/// Tracing with ephemerons requires a 3 phase approach:
///   - Phase One: Trace everything up to an ephemeron (queue found ephemerons)
///   - Phase Two: Trace keys of queued ephemerons. If reachable,
///
/// [Reference]: https://docs.racket-lang.org/reference/ephemerons.html#%28tech._ephemeron%29
pub struct Ephemeron<T: Trace + ?Sized + 'static> {
    key: Cell<NonNull<GcBox<T>>>,
    value: Cell<Option<NonNull<GcBox<T>>>>,
}

impl<T: Trace> Ephemeron<T> {
    pub(crate) fn new_weak(value: T) -> Self {
        assert!(mem::align_of::<GcBox<T>>() > 1);

        unsafe {
            let ptr = GcBox::new(value, GcBoxType::Weak);

            let ephem = Ephemeron {
                key: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
                value: Cell::new(None),
            };
            ephem.set_root();
            ephem
        }
    }

    pub(crate) fn new_weak_pair(key: T, value: Option<T>) -> Self {
        assert!(mem::align_of::<GcBox<T>>() > 1);

        unsafe {
            let key_ptr = GcBox::new(key, GcBoxType::Weak);
            let value = if let Some(v) = value {
                let val_ptr = GcBox::new(v, GcBoxType::Weak);
                Cell::new(Some(NonNull::new_unchecked(val_ptr.as_ptr())))
            } else {
                Cell::new(None)
            };

            let ephem = Ephemeron {
                key: Cell::new(NonNull::new_unchecked(key_ptr.as_ptr())),
                value,
            };
            ephem.set_root();
            ephem
        }
    }

    #[inline]
    pub(crate) fn set_value(&self, value: Option<T>) {
        unsafe {
            if let Some(v) = value {
                let val_ptr = GcBox::new(v, GcBoxType::Weak);
                self.value
                    .set(Some(NonNull::new_unchecked(val_ptr.as_ptr())));
            } else {
                self.value.set(None);
            }
        }
    }
}

impl<T: Trace + ?Sized> Ephemeron<T> {
    #[inline]
    pub(crate) fn weak_from_gc_box(value: NonNull<GcBox<T>>) -> Self {
        unsafe {
            let ephem = Ephemeron {
                key: Cell::new(NonNull::new_unchecked(value.as_ptr())),
                value: Cell::new(None),
            };
            ephem.set_root();
            ephem
        }
    }

    #[inline]
    pub(crate) fn weak_pair_from_gc_boxes(
        key: NonNull<GcBox<T>>,
        value: Option<NonNull<GcBox<T>>>,
    ) -> Self {
        unsafe {
            let value = if let Some(val) = value {
                Cell::new(Some(NonNull::new_unchecked(val.as_ptr())))
            } else {
                Cell::new(None)
            };

            let ephem = Ephemeron {
                key: Cell::new(NonNull::new_unchecked(key.as_ptr())),
                value,
            };
            ephem.set_root();
            ephem
        }
    }

    fn rooted(&self) -> bool {
        self.key.get().as_ptr() as *mut u8 as usize & 1 != 0
    }

    unsafe fn set_root(&self) {
        let ptr = self.key.get().as_ptr();
        let data = ptr as *mut u8;
        let addr = data as isize;
        let ptr = set_data_ptr(ptr, data.wrapping_offset((addr | 1) - addr));
        self.key.set(NonNull::new_unchecked(ptr));
    }

    unsafe fn clear_root(&self) {
        self.key.set(clear_root_bit(self.key.get()));
    }

    #[inline]
    pub(crate) fn is_marked(&self) -> bool {
        self.inner_key().is_marked()
    }

    #[inline]
    fn inner_key_ptr(&self) -> *mut GcBox<T> {
        assert!(finalizer_safe());
        unsafe { clear_root_bit(self.key.get()).as_ptr() }
    }

    #[inline]
    fn inner_value_ptr(&self) -> Option<*mut GcBox<T>> {
        assert!(finalizer_safe());

        if let Some(gc_box) = self.value.get() {
            let val = unsafe { gc_box.as_ptr() };
            Some(val)
        } else {
            None
        }
    }

    #[inline]
    fn inner_key(&self) -> &GcBox<T> {
        unsafe { &*self.inner_key_ptr() }
    }

    #[inline]
    fn inner_value(&self) -> Option<&GcBox<T>> {
        unsafe {
            if let Some(inner_value) = self.inner_value_ptr() {
                Some(&*inner_value)
            } else {
                None
            }
        }
    }

    #[inline]
    pub fn key_value(&self) -> &T {
        self.inner_key().value()
    }

    #[inline]
    pub fn value(&self) -> Option<&T> {
        if let Some(gcbox) = self.inner_value() {
            Some(gcbox.value())
        } else {
            None
        }
    }

    #[inline]
    unsafe fn weak_trace_key(&self, queue: &mut Vec<GcPointer>) {
        self.inner_key().weak_trace_inner(queue)
    }

    #[inline]
    unsafe fn weak_trace_value(&self, queue: &mut Vec<GcPointer>) {
        if let Some(gcbox) = self.inner_value() {
            gcbox.weak_trace_inner(queue)
        }
    }
}

impl<T: Trace + ?Sized> Finalize for Ephemeron<T> {}

unsafe impl<T: Trace + ?Sized> Trace for Ephemeron<T> {
    #[inline]
    unsafe fn trace(&self) {
        /* An ephemeron is never traced with Phase One Trace */
        /* May be traced in phase 3, so this still may need to be implemented */
    }

    #[inline]
    unsafe fn is_marked_ephemeron(&self) -> bool {
        self.is_marked()
    }

    #[inline]
    unsafe fn weak_trace(&self, queue: &mut Vec<GcPointer>) {
        if self.is_marked() {
            self.weak_trace_key(queue);
            self.weak_trace_value(queue);
        }
    }

    #[inline]
    unsafe fn root(&self) {
        // An ephemeron is never rooted in the GcBoxHeader
        assert!(!self.rooted(), "Can't double-root an Ephemeron<T>");

        self.set_root()
    }

    #[inline]
    unsafe fn unroot(&self) {
        // An ephemeron is never rotted in the GcBoxHeader
        assert!(self.rooted(), "Can't double-unroot an Ephemeron");
        self.clear_root();
    }

    #[inline]
    fn finalize_glue(&self) {
        Finalize::finalize(self)
    }
}
