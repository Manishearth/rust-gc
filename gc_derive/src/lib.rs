use quote::quote;
use synstructure::{decl_derive, AddBounds, Structure};

decl_derive!([Trace, attributes(unsafe_ignore_trace)] => derive_trace);

fn derive_trace(mut s: Structure<'_>) -> proc_macro2::TokenStream {
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

    // Generate some code which will fail to compile if the derived type has an
    // unsafe `drop` implementation.
    let (impl_generics, ty_generics, where_clause) = s.ast().generics.split_for_impl();
    let ident = &s.ast().ident;
    let assert_not_drop = quote! {
        // This approach to negative trait assertions is directly copied from
        // `static_assertions` v1.1.0.
        // https://github.com/nvzqz/static-assertions-rs/blob/18bc65a094d890fe1faa5d3ccb70f12b89eabf56/src/assert_impl.rs#L262-L287
        const _: () = {
            // Generic trait with a blanket impl over `()` for all types.
            trait AmbiguousIfDrop<T> {
                fn some_item() {}
            }

            impl<T: ?::std::marker::Sized> AmbiguousIfDrop<()> for T {}

            #[allow(dead_code)]
            struct Invalid;
            impl<T: ?::std::marker::Sized + ::std::ops::Drop> AmbiguousIfDrop<Invalid> for T {}

            // If there is only one specialized trait impl, type inference with
            // `_` can be resolved, and this will compile.
            //
            // Fails to compile if `AmbiguousIfDrop<Invalid>` is implemented for
            // our type.
            #[allow(dead_code)]
            fn assert_not_drop #impl_generics () #where_clause {
                let _ = <#ident #ty_generics as AmbiguousIfDrop<_>>::some_item;
            }
        };
    };

    quote! {
        #trace_impl
        #assert_not_drop
    }
}

decl_derive!([Finalize] => derive_finalize);

#[allow(clippy::needless_pass_by_value)]
fn derive_finalize(s: Structure<'_>) -> proc_macro2::TokenStream {
    s.unbound_impl(quote!(::gc::Finalize), quote!())
}
