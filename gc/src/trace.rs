/// The Finalize trait. Can be specialized for a specific type to define
/// finalization logic for that type.
pub trait Finalize {
    fn finalize(&self);
}

impl<T: ?Sized> Finalize for T {
    // XXX: Should this function somehow tell its caller (which is presumably
    // the GC runtime) that it did nothing?
    #[inline]
    default fn finalize(&self) {}
}

/// The Trace trait, which needs to be implemented on garbage-collected objects.
pub unsafe trait Trace : Finalize {
    /// Marks all contained `Gc`s.
    unsafe fn trace(&self);

    /// Increments the root-count of all contained `Gc`s.
    unsafe fn root(&self);

    /// Decrements the root-count of all contained `Gc`s.
    unsafe fn unroot(&self);

    /// Runs Finalize::finalize() on this object and all
    /// contained subobjects
    fn finalize_glue(&self);
}

/// This rule implements the trace methods with empty implementations.
///
/// Use this for marking types as not containing any `Trace` types.
#[macro_export]
macro_rules! unsafe_empty_trace {
    () => {
        #[inline]
        unsafe fn trace(&self) {}
        #[inline]
        unsafe fn root(&self) {}
        #[inline]
        unsafe fn unroot(&self) {}
        #[inline]
        fn finalize_glue(&self) {
            $crate::Finalize::finalize(self)
        }
    }
}

/// This rule implements the trace method.
///
/// You define a `this` parameter name and pass in a body, which should call `mark` on every
/// traceable element inside the body. The mark implementation will automatically delegate to the
/// correct method on the argument.
#[macro_export]
macro_rules! custom_trace {
    ($this:ident, $body:expr) => {
        #[inline]
        unsafe fn trace(&self) {
            #[inline]
            unsafe fn mark<T: $crate::Trace>(it: &T) {
                $crate::Trace::trace(it);
            }
            let $this = self;
            $body
        }
        #[inline]
        unsafe fn root(&self) {
            #[inline]
            unsafe fn mark<T: $crate::Trace>(it: &T) {
                $crate::Trace::root(it);
            }
            let $this = self;
            $body
        }
        #[inline]
        unsafe fn unroot(&self) {
            #[inline]
            unsafe fn mark<T: $crate::Trace>(it: &T) {
                $crate::Trace::unroot(it);
            }
            let $this = self;
            $body
        }
        #[inline]
        fn finalize_glue(&self) {
            $crate::Finalize::finalize(self);
            #[inline]
            fn mark<T: $crate::Trace>(it: &T) {
                $crate::Trace::finalize_glue(it);
            }
            let $this = self;
            $body
        }
    }
}

unsafe impl<T: ?Sized> Trace for &'static T {
    unsafe_empty_trace!();
}

unsafe impl Trace for usize { unsafe_empty_trace!(); }
unsafe impl Trace for bool { unsafe_empty_trace!(); }
unsafe impl Trace for i8  { unsafe_empty_trace!(); }
unsafe impl Trace for u8  { unsafe_empty_trace!(); }
unsafe impl Trace for i16 { unsafe_empty_trace!(); }
unsafe impl Trace for u16 { unsafe_empty_trace!(); }
unsafe impl Trace for i32 { unsafe_empty_trace!(); }
unsafe impl Trace for u32 { unsafe_empty_trace!(); }
unsafe impl Trace for i64 { unsafe_empty_trace!(); }
unsafe impl Trace for u64 { unsafe_empty_trace!(); }

unsafe impl Trace for f32 { unsafe_empty_trace!(); }
unsafe impl Trace for f64 { unsafe_empty_trace!(); }

unsafe impl Trace for String { unsafe_empty_trace!(); }

unsafe impl<T: Trace> Trace for Box<T> {
    custom_trace!(this, {
        mark(&**this);
    });
}

unsafe impl<T: Trace> Trace for Vec<T> {
    custom_trace!(this, {
        for e in this {
            mark(e);
        }
    });
}

unsafe impl<T: Trace> Trace for Option<T> {
    custom_trace!(this, {
        if let Some(ref v) = *this {
            mark(v);
        }
    });
}
