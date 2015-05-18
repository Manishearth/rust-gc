/// The Trace trait which needs to be implemented on garbage collected objects
pub trait Trace {
    /// Mark all contained Gcs
    unsafe fn trace(&self);
    // Next two should be unsafe (see #1)
    /// Increment the root-count of all contained Gcs
    unsafe fn root(&self);
    /// Decrement the root-count of all contained Gcs
    unsafe fn unroot(&self);
}

impl<T> Trace for &'static T {
    unsafe fn trace(&self) {}
    unsafe fn root(&self) {}
    unsafe fn unroot(&self) {}
}

impl<'a, T: Trace> Trace for Box<T> {
    unsafe fn trace(&self) {
        (**self).trace();
    }

    unsafe fn root(&self) {
        (**self).root();
    }

    unsafe fn unroot(&self) {
        (**self).unroot();
    }
}

impl<'a, T: Trace> Trace for Vec<T> {
    unsafe fn trace(&self) {
        for e in self {
            e.trace();
        }
    }

    unsafe fn root(&self) {
        for e in self {
            e.root();
        }
    }

    unsafe fn unroot(&self) {
        for e in self {
            e.unroot();
        }
    }
}

impl<'a, T: Trace> Trace for Option<T> {
    unsafe fn trace(&self) {
        if let Some(ref v) = *self {
            v.trace();
        }
    }
    unsafe fn root(&self) {
        if let Some(ref v) = *self {
            v.root();
        }
    }
    unsafe fn unroot(&self) {
        if let Some(ref v) = *self {
            v.unroot();
        }
    }
}
