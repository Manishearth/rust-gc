#[allow(dead_code)]
mod static_tests {
    use gc::{EmptyTrace, Finalize, Gc, Trace};
    use std::rc::Rc;

    #[derive(EmptyTrace)]
    struct StructWithEmptyTrace(Rc<Option<StructWithEmptyTrace>>);

    #[derive(Trace, Finalize)]
    struct Traceable<T> {
        #[empty_trace]
        a: StructWithEmptyTrace,

        #[empty_trace]
        b: T,
    }

    fn test_empty_trace() {
        Gc::new(Traceable {
            a: StructWithEmptyTrace(Rc::new(None)),
            b: Rc::new(String::new()),
        });
    }
}
