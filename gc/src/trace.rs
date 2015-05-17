/// The Trace trait which needs to be implemented on garbage collected objects
pub trait Trace {
    /// Mark all contained Gcs
    fn trace(&self);
    // Next two should be unsafe (see #1)
    /// Increment the root-count of all contained Gcs
    fn root(&self);
    /// Decrement the root-count of all contained Gcs
    fn unroot(&self);
}

impl<T> Trace for &'static T {
    fn trace(&self) {}
    fn root(&self) {}
    fn unroot(&self) {}
}

impl<'a, T: Trace> Trace for Box<T> {
    fn trace(&self) {
        (**self).trace();
    }

    fn root(&self) {
        (**self).root();
    }

    fn unroot(&self) {
        (**self).unroot();
    }
}

impl<'a, T: Trace> Trace for Vec<T> {
    fn trace(&self) {
        for e in self {
            e.trace();
        }
    }

    fn root(&self) {
        for e in self {
            e.root();
        }
    }

    fn unroot(&self) {
        for e in self {
            e.unroot();
        }
    }
}
