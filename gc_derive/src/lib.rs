extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate synstructure;
#[macro_use]
extern crate quote;

decl_derive!([Trace, attributes(unsafe_ignore_trace)] => derive_trace);

fn derive_trace(mut s: synstructure::Structure) -> quote::Tokens {
    s.filter(|bi| !bi.ast().attrs.iter().any(|attr| attr.name() == "unsafe_ignore_trace"));
    let trace_body = s.each(|bi| quote!(mark(#bi)));

    let trace_impl = s.unsafe_bound_impl("::gc::Trace", quote! {
        #[inline] unsafe fn trace(&self) {
            #[allow(dead_code)]
            #[inline]
            unsafe fn mark<T: ::gc::Trace>(it: &T) {
                ::gc::Trace::trace(it);
            }
            match *self { #trace_body }
        }
        #[inline] unsafe fn root(&self) {
            #[allow(dead_code)]
            #[inline]
            unsafe fn mark<T: ::gc::Trace>(it: &T) {
                ::gc::Trace::root(it);
            }
            match *self { #trace_body }
        }
        #[inline] unsafe fn unroot(&self) {
            #[allow(dead_code)]
            #[inline]
            unsafe fn mark<T: ::gc::Trace>(it: &T) {
                ::gc::Trace::unroot(it);
            }
            match *self { #trace_body }
        }
        #[inline] fn finalize_glue(&self) {
            #[allow(dead_code)]
            #[inline]
            fn mark<T: ::gc::Trace>(it: &T) {
                ::gc::Trace::finalize_glue(it);
            }
            match *self { #trace_body }
            ::gc::Finalize::finalize(self);
        }
    });

    // We also implement drop to prevent unsafe drop implementations on this
    // type and encourage people to use Finalize. This implementation will
    // call `Finalize::finalize` if it is safe to do so.
    let drop_impl = s.unbound_impl("::std::ops::Drop", quote! {
        fn drop(&mut self) {
            if ::gc::finalizer_safe() {
                ::gc::Finalize::finalize(self);
            }
        }
    });

    quote! {
        #trace_impl
        #drop_impl
    }
}

decl_derive!([Finalize] => derive_finalize);

fn derive_finalize(s: synstructure::Structure) -> quote::Tokens {
    s.unbound_impl("::gc::Finalize", quote!())
}
