use quote::quote;
use synstructure::{decl_derive, AddBounds, BindStyle, Structure};

decl_derive!([Trace, attributes(unsafe_ignore_trace, trivially_drop)] => derive_trace);

fn derive_trace(mut s: Structure<'_>) -> proc_macro2::TokenStream {
    let is_trivially_drop = s
        .ast()
        .attrs
        .iter()
        .any(|attr| attr.path.is_ident("trivially_drop"));
    s.filter(|bi| {
        !bi.ast()
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("unsafe_ignore_trace"))
    });
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

    if !is_trivially_drop {
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

        quote! {
            #trace_impl
            #drop_impl
        }
    } else {
        s.bind_with(|_| BindStyle::Move);
        let trivially_drop_body = s.each(|_| quote! {});
        let finalize_impl = s.bound_impl(
            quote!(::gc::Finalize),
            quote!(
                fn finalize(&self) {
                    let _trivially_drop = |t: Self| match t { #trivially_drop_body };
                }
            ),
        );

        quote! {
            #trace_impl
            #finalize_impl
        }
    }
}

decl_derive!([Finalize] => derive_finalize);

#[allow(clippy::needless_pass_by_value)]
fn derive_finalize(s: Structure<'_>) -> proc_macro2::TokenStream {
    s.unbound_impl(quote!(::gc::Finalize), quote!())
}
