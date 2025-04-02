use quote::{format_ident, quote, ToTokens};
use syn::spanned::Spanned;
use syn::{parse_quote_spanned, GenericParam, WherePredicate};
use synstructure::{decl_derive, AddBounds, Structure, VariantInfo};

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

fn derive_empty_trace(s: Structure<'_>) -> proc_macro2::TokenStream {
    let s_ast = &s.ast();
    let name = &s_ast.ident;
    let temp_name = format_ident!("_{name}");
    let params = &s_ast.generics.params;
    let param_names = params
        .iter()
        .map(|p| match p {
            GenericParam::Lifetime(p) => p.to_token_stream(),
            GenericParam::Type(p) => p.ident.to_token_stream(),
            GenericParam::Const(p) => p.ident.to_token_stream(),
        })
        .collect::<Vec<_>>();
    let where_predicates = &s_ast
        .generics
        .where_clause
        .iter()
        .flat_map(|wc| &wc.predicates)
        .collect::<Vec<_>>();

    // Require that all bindings implement `EmptyTrace`
    let bindings = s.variants().iter().flat_map(VariantInfo::bindings);
    let additional_where_predicates: Vec<WherePredicate> = bindings
        .map(|bi| {
            let ty = &bi.ast().ty;
            let span = ty.span();
            parse_quote_spanned! { span=> #ty: ::gc::EmptyTrace }
        })
        .collect();

    // If any bindings in `s` refer to `s` itself then trait resolution could run into a cycle through our generated where predicates.
    // We solve this with the following hack:
    // Locally, we rename `s` and replace it with a temporary type of the same shape.
    // That type unconditionally implements `EmptyTrace`, which might, technically, be unsafe but is fine since we never instantiate that type.
    // Its only purpose is to stand in for `s` inside the generated predicates in order to break the cycle.
    quote! {
        const _: () = {
            type #temp_name<#params> = #name<#(#param_names),*>;
            {
                #s_ast

                unsafe impl<#params> ::gc::EmptyTrace
                for #name<#(#param_names),*>
                where
                    #(#where_predicates),*
                {}

                unsafe impl<#params> ::gc::EmptyTrace
                for #temp_name<#(#param_names),*>
                where
                    #(#where_predicates),*
                    #(#additional_where_predicates),*
                {}
            }
        };
    }
}
