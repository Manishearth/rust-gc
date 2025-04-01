#[allow(dead_code)]
mod static_tests {
    use gc::Gc;
    use gc_derive::{EmptyTrace, Finalize, Trace};
    use std::rc::Rc;
    
    #[derive(EmptyTrace)]
    struct StructWithEmptyTrace(Rc<String>);

    #[derive(Trace, Finalize)]
    struct Traceable<T> {
        #[empty_trace]
        a: StructWithEmptyTrace,

        #[empty_trace]
        b: T,
    }

    fn test_empty_trace() {
        let x = Rc::new(String::new());
        Gc::new(Traceable {
            a: StructWithEmptyTrace(x.clone()),
            b: x.clone(),
        });
    }
}
