/// The Trace trait which needs to be implemented on garbage collected objects
pub trait Trace {
    /// Mark all contained Gcs
    fn trace(&self);
    /// Increment the root-count of all contained Gcs
    unsafe fn root(&self);
    /// Decrement the root-count of all contained Gcs
    unsafe fn unroot(&self);
}

/*
impl<'a, T> Trace for &'a T {
    fn trace(&self) {}
    unsafe fn root(&self) {}
    unsafe fn unroot(&self) {}
}
*/

impl<'a, T: Trace> Trace for Box<T> {
    fn trace(&self) {
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
    fn trace(&self) {
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
