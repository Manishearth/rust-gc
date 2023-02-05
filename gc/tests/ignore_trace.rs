use gc::{force_collect, Finalize, Gc, Trace};

#[derive(Finalize, Trace)]
struct S(#[unsafe_ignore_trace] Gc<()>);

/// Using `#[unsafe_ignore_trace]` on a `Gc` may inhibit collection of
/// cycles through that `Gc`, but it should not result in panics.
#[test]
fn ignore_trace_gc() {
    Gc::new(S(Gc::new(())));
    force_collect();
}
