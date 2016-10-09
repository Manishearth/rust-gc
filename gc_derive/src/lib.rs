#![feature(proc_macro, proc_macro_lib)]

extern crate proc_macro;
extern crate syn;
extern crate synstructure;
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use synstructure::BindStyle;

#[proc_macro_derive(Trace)]
pub fn derive_trace(input: TokenStream) -> TokenStream {
    let source = input.to_string();
    let mut ast = syn::parse_macro_input(&source).unwrap();

    let trace = synstructure::each_field(&mut ast, BindStyle::Ref, |bi| {
        // Check if this field is annotated with an #[unsafe_ignore_trace], and
        // remove the attribute if it is present.
        let attr_cnt = bi.field.attrs.len();
        bi.field.attrs.retain(|attr| attr.name() != "unsafe_ignore_trace");

        if bi.field.attrs.len() != attr_cnt {
            quote::Tokens::new()
        } else {
            quote!(mark(#bi);)
        }
    });

    // Build the output tokens
    let name = &ast.ident;
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();
    let result = quote! {
        // Original struct
        #ast

        unsafe impl #impl_generics ::gc::Trace for #name #ty_generics #where_clause {
            #[inline] unsafe fn trace(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace>(it: &T) {
                    ::gc::Trace::trace(it);
                }
                match *self { #trace }
            }
            #[inline] unsafe fn root(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace>(it: &T) {
                    ::gc::Trace::root(it);
                }
                match *self { #trace }
            }
            #[inline] unsafe fn unroot(&self) {
                #[allow(dead_code)]
                #[inline]
                unsafe fn mark<T: ::gc::Trace>(it: &T) {
                    ::gc::Trace::unroot(it);
                }
                match *self { #trace }
            }
            #[inline] fn finalize_glue(&self) {
                #[allow(dead_code)]
                #[inline]
                fn mark<T: ::gc::Trace>(it: &T) {
                    ::gc::Trace::finalize_glue(it);
                }
                match *self { #trace }
                ::gc::Finalize::finalize(self);
            }
        }

        // We also implement drop to prevent unsafe drop implementations on this
        // type and encourage people to use Finalize. This implementation will
        // call `Finalize::finalize` if it is safe to do so.
        impl #impl_generics ::std::ops::Drop for #name #ty_generics #where_clause {
            fn drop(&mut self) {
                if ::gc::finalizer_safe() {
                    ::gc::Finalize::finalize(self);
                }
            }
        }
    };

    // Generate the final value as a TokenStream and return it
    result.to_string().parse().unwrap()
}
