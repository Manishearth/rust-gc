use quote::quote;
use syn::parse_quote_spanned;
use syn::spanned::Spanned;
use synstructure::{decl_derive, AddBounds, Structure};

decl_derive!([Trace, attributes(unsafe_ignore_trace, empty_trace)] => derive_trace);

fn derive_trace(mut s: Structure<'_>) -> proc_macro2::TokenStream {
    s.filter(|bi| {
        !bi.ast()
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("unsafe_ignore_trace"))
    });

    // We also implement drop to prevent unsafe drop implementations on this
    // type and encourage people to use Finalize. This implementation will
    // call `Finalize::finalize` if it is safe to do so.
    let drop_impl = s.unbound_impl(
        quote!(::std::ops::Drop),
        quote! {
            fn drop(&mut self) {
                if ::gc::finalizer_safe() {
                    ::gc::Finalize::finalize(self);
                }
            }
        },
    );

    // Separate all the bindings that were annotated with `#[empty_trace]`
    let empty_trace_bindings = s.drain_filter(|bi| {
        bi.ast()
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("empty_trace"))
    });

    // Require the annotated bindings to implement `EmptyTrace`
    for variant in empty_trace_bindings.variants() {
        for binding in variant.bindings() {
            let ty = &binding.ast().ty;
            s.add_where_predicate(parse_quote_spanned!(ty.span()=> #ty: ::gc::EmptyTrace));
        }
    }

    let trace_body = s.each(|bi| quote!(mark(#bi)));

    s.add_bounds(AddBounds::Fields);
    let trace_impl = s.unsafe_bound_impl(
        quote!(::gc::Trace),
        quote! {
            #[inline] unsafe fn trace(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace + ?Sized>(it: &T) {
                    ::gc::Trace::trace(it);
                }
                match *self { #trace_body }
            }
            #[inline] unsafe fn root(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace + ?Sized>(it: &T) {
                    ::gc::Trace::root(it);
                }
                match *self { #trace_body }
            }
            #[inline] unsafe fn unroot(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace + ?Sized>(it: &T) {
                    ::gc::Trace::unroot(it);
                }
                match *self { #trace_body }
            }
            #[inline] fn finalize_glue(&self) {
                ::gc::Finalize::finalize(self);
                #[allow(dead_code)]
                #[inline]
                fn mark<T: ::gc::Trace + ?Sized>(it: &T) {
                    ::gc::Trace::finalize_glue(it);
                }
                match *self { #trace_body }
            }
        },
    );

    quote! {
        #trace_impl
        #drop_impl
    }
}

decl_derive!([Finalize] => derive_finalize);

#[allow(clippy::needless_pass_by_value)]
fn derive_finalize(s: Structure<'_>) -> proc_macro2::TokenStream {
    s.unbound_impl(quote!(::gc::Finalize), quote!())
}

decl_derive!([EmptyTrace] => derive_empty_trace);

// TODO: Does not work on self-referential types
fn derive_empty_trace(mut s: Structure<'_>) -> proc_macro2::TokenStream {
    // Add where bounds for all bindings manually because synstructure only adds them if they depend on one of the parameters.
    let mut where_predicates = Vec::new();
    for v in s.variants() {
        for bi in v.bindings() {
            let ty = &bi.ast().ty;
            let span = ty.span();
            where_predicates.push(parse_quote_spanned! { span=> #ty: ::gc::EmptyTrace });
        }
    }
    for p in where_predicates {
        s.add_where_predicate(p);
    }
    s.add_bounds(AddBounds::None);
    s.unsafe_bound_impl(quote! { ::gc::EmptyTrace }, quote! {})
}
