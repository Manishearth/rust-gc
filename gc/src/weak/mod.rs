use crate::gc::GcBox;
pub use crate::gc::{finalizer_safe, force_collect};
use crate::set_data_ptr;
pub use crate::trace::{Finalize, Trace};
use std::ptr::NonNull;

pub(crate) mod ephemeron;
pub mod pair;
pub mod weak_gc;

pub(crate) use ephemeron::Ephemeron;
pub use pair::WeakPair;
pub use weak_gc::WeakGc;

pub(crate) unsafe fn clear_root_bit<T: ?Sized + Trace>(
    ptr: NonNull<GcBox<Ephemeron<T>>>,
) -> NonNull<GcBox<Ephemeron<T>>> {
    let ptr = ptr.as_ptr();
    let data = ptr as *mut u8;
    let addr = data as isize;
    let ptr = set_data_ptr(ptr, data.wrapping_offset((addr & !1) - addr));
    NonNull::new_unchecked(ptr)
}
