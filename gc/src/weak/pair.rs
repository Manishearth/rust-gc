pub use crate::gc::{finalizer_safe, force_collect};
use crate::gc::{GcBox, GcBoxType};
pub use crate::trace::{Finalize, Trace};
use crate::weak::{clear_root_bit, Ephemeron};
use crate::{set_data_ptr, GcPointer};
use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem;
use std::ops::Deref;
use std::ptr::NonNull;
use std::rc::Rc;

//////////////
// WeakPair //
//////////////

// The WeakPair struct is a garbage collected pointer to an Ephemeron<K, V>
pub struct WeakPair<T: Trace + ?Sized + 'static> {
    ptr_root: Cell<NonNull<GcBox<Ephemeron<T>>>>,
    marker: PhantomData<Rc<T>>,
}

impl<T: Trace> WeakPair<T> {
    /// Crate a new Weak type Gc
    ///
    /// This method can trigger a collection    
    pub fn new(key: T, value: Option<T>) -> Self {
        assert!(mem::align_of::<GcBox<T>>() > 1);

        unsafe {
            // Allocate the memory for the object
            let eph_value = Ephemeron::new_weak_pair(key, value);
            let ptr = GcBox::new(eph_value, GcBoxType::Ephemeron);

            (*ptr.as_ptr()).value().unroot();
            let weak_gc = WeakPair {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
                marker: PhantomData,
            };
            weak_gc.set_root();
            weak_gc
        }
    }

    #[inline]
    pub fn set_value(&self, value: Option<T>) {
        self.inner().value().set_value(value)
    }
}

impl<T: Trace + ?Sized> WeakPair<T> {
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
    fn inner_ptr(&self) -> *mut GcBox<Ephemeron<T>> {
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
    fn inner(&self) -> &GcBox<Ephemeron<T>> {
        unsafe { &*self.inner_ptr() }
    }
}

impl<T: Trace + ?Sized> WeakPair<T> {
    #[inline]
    pub fn key_value(&self) -> &T {
        self.inner().value().key_value()
    }

    #[inline]
    pub fn value(&self) -> Option<&T> {
        self.inner().value().value()
    }

    #[inline]
    pub fn value_tuple(&self) -> (&T, Option<&T>) {
        (self.key_value(), self.value())
    }

    #[inline]
    pub(crate) fn from_gc_boxes(key: NonNull<GcBox<T>>, value: Option<NonNull<GcBox<T>>>) -> Self {
        unsafe {
            let eph = Ephemeron::weak_pair_from_gc_boxes(key, value);
            let ptr = GcBox::new(eph, GcBoxType::Ephemeron);

            let weak_gc = WeakPair {
                ptr_root: Cell::new(NonNull::new_unchecked(ptr.as_ptr())),
                marker: PhantomData,
            };
            weak_gc.set_root();
            weak_gc
        }
    }
}

impl<T: Trace + ?Sized> Finalize for WeakPair<T> {}

unsafe impl<T: Trace + ?Sized> Trace for WeakPair<T> {
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
        assert!(!self.rooted(), "Can't double-root a WeakPair<T>");
        self.set_root();
    }

    #[inline]
    unsafe fn unroot(&self) {
        assert!(self.rooted(), "Can't double-unroot a WeakPair<T>");
        self.clear_root();
    }

    #[inline]
    fn finalize_glue(&self) {
        Finalize::finalize(self)
    }
}

impl<T: Trace + ?Sized> Clone for WeakPair<T> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            let weak_gc = WeakPair {
                ptr_root: Cell::new(self.ptr_root.get()),
                marker: PhantomData,
            };
            weak_gc.set_root();
            weak_gc
        }
    }
}

impl<T: Trace + ?Sized> Deref for WeakPair<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner().value().key_value()
    }
}

impl<T: Trace + Default> Default for WeakPair<T> {
    #[inline]
    fn default() -> Self {
        Self::new(Default::default(), Default::default())
    }
}

impl<T: Trace + ?Sized + PartialEq> PartialEq for WeakPair<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Trace + ?Sized + Eq> Eq for WeakPair<T> {}

impl<T: Trace + ?Sized + PartialOrd> PartialOrd for WeakPair<T> {
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

impl<T: Trace + ?Sized + Ord> Ord for WeakPair<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: Trace + ?Sized + Hash> Hash for WeakPair<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: Trace + ?Sized + Display> Display for WeakPair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized + Debug> Debug for WeakPair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<T: Trace + ?Sized> fmt::Pointer for WeakPair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.inner(), f)
    }
}

// TODO: implement FROM trait for WeakPair

impl<T: Trace + ?Sized> std::borrow::Borrow<T> for WeakPair<T> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: Trace + ?Sized> std::convert::AsRef<T> for WeakPair<T> {
    fn as_ref(&self) -> &T {
        &**self
    }
}
