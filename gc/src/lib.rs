#![feature(std_misc, optin_builtin_traits, core)]

mod trace;
pub mod gc;
mod gc_internals;

pub use trace::{Trace, Tracer};
pub use gc::{Gc, GcCell};

pub fn gc_force_collect() { gc_internals::force_collect(); }
