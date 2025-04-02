use std::rc::Rc;

/// A marker trait for types that don't require tracing.
/// TODO: Safety conditions
pub unsafe trait EmptyTrace {}

unsafe impl EmptyTrace for String {}

unsafe impl<T: EmptyTrace> EmptyTrace for Rc<T> {}

unsafe impl<T: EmptyTrace> EmptyTrace for Box<T> {}

unsafe impl<T: EmptyTrace> EmptyTrace for Option<T> {}