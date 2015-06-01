pub trait Tracer {
    unsafe fn traverse<T: Trace>(&self, obj: &T);
}

/// The Trace trait must be implemented for every garbage-collectable object
/// Only the _trace method should be overridden, unless you are doing something
/// super super weird.
///
/// This trait can be auto-derived using #[derive(Trace)] if you are using the
/// gc_plugin compiler plugin in your program.
pub trait Trace {
    /// This method should be overridden for every implementer of Trace.
    /// It is called by the default implementations of the other methods.
    ///
    /// tracer.traverse() should be called on every collectable element of
    /// the object in question implementing Trace.
    ///
    /// Generally avoid implementing this yourself, and prefer using #[derive(Trace)]
    /// to avoid unsafety.
    unsafe fn _trace<T: Tracer>(&self, tracer: T);

    unsafe fn _gc_mark(&self) {
        struct MarkTracer;
        impl Tracer for MarkTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._gc_mark()
            }
        }
        self._trace(MarkTracer);
    }
    unsafe fn _gc_root(&self) {
        struct RootTracer;
        impl Tracer for RootTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._gc_root()
            }
        }
        self._trace(RootTracer);
    }
    unsafe fn _gc_unroot(&self) {
        struct UnrootTracer;
        impl Tracer for UnrootTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._gc_unroot()
            }
        }
        self._trace(UnrootTracer);
    }
    unsafe fn _cgc_mark(&self) {
        struct MarkTracer;
        impl Tracer for MarkTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._cgc_mark()
            }
        }
        self._trace(MarkTracer);
    }
    unsafe fn _cgc_root(&self) {
        struct RootTracer;
        impl Tracer for RootTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._cgc_root()
            }
        }
        self._trace(RootTracer);
    }
    unsafe fn _cgc_unroot(&self) {
        struct UnrootTracer;
        impl Tracer for UnrootTracer {
            #[inline(always)]
            unsafe fn traverse<T: Trace>(&self, obj: &T) {
                obj._cgc_unroot()
            }
        }
        self._trace(UnrootTracer);
    }
}

impl<U> Trace for &'static U {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}

impl Trace for i8 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for u8 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for i16 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for u16 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for i32 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for u32 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for i64 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for u64 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}

impl Trace for f32 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}
impl Trace for f64 {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}

impl Trace for String {
    unsafe fn _trace<T: Tracer>(&self, _: T) {}
}

impl<U: Trace> Trace for Box<U> {
    unsafe fn _trace<T: Tracer>(&self, t: T) {
        t.traverse(&**self);
    }
}

impl<U: Trace> Trace for Vec<U> {
    unsafe fn _trace<T: Tracer>(&self, t: T) {
        for e in self {
            t.traverse(e);
        }
    }
}

impl<U: Trace> Trace for Option<U> {
    unsafe fn _trace<T: Tracer>(&self, t: T) {
        if let Some(ref v) = *self {
            t.traverse(v);
        }
    }
}
