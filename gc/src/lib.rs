#![feature(optin_builtin_traits, unsize, coerce_unsized, borrow_state)]

#[macro_use]
extern crate lazy_static;

mod trace;

pub mod gc;
mod gc_internals;

pub mod cgc;
mod cgc_internals;

pub use trace::{Trace, Tracer};
pub use gc::{Gc, GcCell};
pub use cgc::{Cgc};

pub fn gc_force_collect() { gc_internals::force_collect(); }
pub fn cgc_force_collect() { cgc_internals::force_collect(); }
