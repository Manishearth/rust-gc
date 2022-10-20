pub use crate::gc::{finalizer_safe, force_collect};
use crate::gc::{GcBox, GcBoxType};
pub use crate::trace::{Finalize, Trace};
use crate::weak::{clear_root_bit, Ephemeron};
use crate::{set_data_ptr, GcPointer};
use std::cell::Cell;
use std::fmt;
use std::mem;
use std::ops::Deref;
use std::ptr::NonNull;

//////////////
// WeakPair //
//////////////

// The WeakPair struct is a garbage collected pointer to an Ephemeron<K, V>
pub struct WeakPair<K: Trace + ?Sized + 'static, V: Trace + ?Sized + 'static> {
    ptr_root: Cell<NonNull<GcBox<Ephemeron<K, V>>>>,
}

impl<K: Trace, V: Trace> WeakPair<K, V> {
    /// Crate a new Weak type Gc
    ///
    /// This method can trigger a collection    
    pub fn new(key: K, value: Option<V>) -> Self {
        assert!(mem::align_of::<GcBox<V>>() > 1);

        unsafe {
            // Allocate the memory for the object
            let eph_value = Ephemeron::new_weak_pair(key, value);
            let ptr = GcBox::new(eph_value, GcBoxType::Ephemeron);

            (*ptr.as_ptr()).value().unroot();
            let weak_gc = WeakPair {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
            };
            weak_gc.set_root();
            weak_gc
        }
    }

    #[inline]
    pub fn set_value(&self, value: V) {
        self.inner().value().set_value(value)
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> WeakPair<K,V> {
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
    fn inner_ptr(&self) -> *mut GcBox<Ephemeron<K,V>> {
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
    fn inner(&self) -> &GcBox<Ephemeron<K, V>> {
        unsafe { &*self.inner_ptr() }
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> WeakPair<K,V> {
    #[inline]
    pub fn key_value(&self) -> &K {
        self.inner().value().key_value()
    }

    #[inline]
    pub fn value(&self) -> Option<&V> {
        self.inner().value().value()
    }

    #[inline]
    pub fn value_tuple(&self) -> (&K, Option<&V>) {
        (self.key_value(), self.value())
    }

    #[inline]
    pub fn from_gc_pair(key: NonNull<GcBox<K>>, value: Option<NonNull<GcBox<V>>>) -> Self {
        unsafe {
            let eph = Ephemeron::new_pair_from_gc_pointers(key, value);
            let ptr = GcBox::new(eph, GcBoxType::Ephemeron);

            let weak_gc = WeakPair {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
            };
            weak_gc.set_root();
            weak_gc
        }
    }
}

impl<K: Trace + ?Sized, V: Trace> WeakPair<K, V> {
    #[inline]
    pub(crate) fn from_gc_value_pair(key:NonNull<GcBox<K>>, value: Option<V>) -> Self {
        unsafe {
            let value_ptr = if let Some(v) = value {
                let gcbox = GcBox::new(v, GcBoxType::Weak);
                Some(NonNull::new_unchecked(gcbox.as_ptr()))
            } else {
                None
            };

            let eph = Ephemeron::new_pair_from_gc_pointers(key, value_ptr);
            let ptr = GcBox::new(eph, GcBoxType::Ephemeron);

            let weak_pair = WeakPair {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
            };
            weak_pair.set_root();
            weak_pair
        }
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> Finalize for WeakPair<K,V> {}

unsafe impl<K: Trace + ?Sized, V: Trace + ?Sized> Trace for WeakPair<K,V> {
    #[inline]
    unsafe fn trace(&self) {
        // Set the strong reference here to false in the case that a trace has run and no
        // strong refs exist.
        self.inner().trace_inner();
    }

    unsafe fn is_marked_ephemeron(&self) -> bool {
        // This is technically an Ephemeron wrapper.
        // Returning false to ensure that only an Ephemeron<T> returns true
        false
    }

    unsafe fn weak_trace(&self, queue: &mut Vec<GcPointer>) {
        // WeakPair is an Ephemeron wrapper, so we know the inner GcBox must contain an
        // an Ephemeron. So we push the Ephemeron onto the Ephemeron queue to be checked
        // by the collector
        queue.push(self.ptr_root.get())
    }

    #[inline]
    unsafe fn root(&self) {
        assert!(!self.rooted(), "Can't double-root a WeakPair<K,V>");
        self.set_root();
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.rooted(), "Can't double-unroot a WeakPair<K,V>");
        self.clear_root();
    }

    #[inline]
    fn finalize_glue(&self) {
        Finalize::finalize(self)
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> Clone for WeakPair<K,V> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            let weak_gc = WeakPair {
                ptr_root: Cell::new(self.ptr_root.get()),
            };
            weak_gc.set_root();
            weak_gc
        }
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> Deref for WeakPair<K,V> {
    type Target = K;

    #[inline]
    fn deref(&self) -> &K {
        &self.inner().value().key_value()
    }
}

impl<K: Trace + Default, V: Trace + Default> Default for WeakPair<K,V> {
    #[inline]
    fn default() -> Self {
        Self::new(Default::default(), Default::default())
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> fmt::Pointer for WeakPair<K,V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.inner(), f)
    }
}

// TODO: implement FROM trait for WeakPair

impl<K: Trace + ?Sized, V: Trace + ?Sized> std::borrow::Borrow<K> for WeakPair<K,V> {
    fn borrow(&self) -> &K {
        &**self
    }
}

impl<K: Trace + ?Sized, V: Trace + ?Sized> std::convert::AsRef<K> for WeakPair<K,V> {
    fn as_ref(&self) -> &K {
        &**self
    }
}
