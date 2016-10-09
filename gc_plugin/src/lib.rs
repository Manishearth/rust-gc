#![feature(plugin_registrar, box_syntax)]
#![feature(rustc_private)]

#[macro_use]
extern crate syntax;
extern crate syntax_ext;
#[macro_use]
extern crate rustc;
extern crate rustc_plugin;


use rustc_plugin::Registry;
use syntax::parse::token::intern;

use syntax::ext::base::{Annotatable, ExtCtxt, MultiDecorator};
use syntax::codemap::Span;
use syntax::ptr::P;
use syntax::ast::{MetaItem, Expr, Mutability};
use syntax::ext::build::AstBuilder;
use syntax_ext::deriving::generic::{combine_substructure, EnumMatching, FieldInfo, MethodDef, Struct, Substructure, TraitDef, ty};


#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
    reg.register_syntax_extension(intern("derive_Trace"), MultiDecorator(box expand_trace))
}

pub fn expand_trace(cx: &mut ExtCtxt, span: Span, mitem: &MetaItem, item: &Annotatable, push: &mut FnMut(Annotatable)) {
    let trait_def = TraitDef {
        span: span,
        attributes: Vec::new(),
        path: ty::Path::new(vec!("gc", "Trace")),
        additional_bounds: Vec::new(),
        generics: ty::LifetimeBounds::empty(),
        methods: vec![
            MethodDef {
                name: "trace",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![], // todo: handle inlining
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure),
                unify_fieldless_variants: false,
            },
            MethodDef {
                name: "root",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure),
                unify_fieldless_variants: false,
            },
            MethodDef {
                name: "unroot",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure),
                unify_fieldless_variants: false,
            },
            MethodDef {
                name: "finalize_glue",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: false,
                combine_substructure: combine_substructure(box finalize_substructure),
                unify_fieldless_variants: false,
            }
        ],
        associated_types: vec![],
        is_unsafe: true,
        supports_unions: false,
    };
    trait_def.expand(cx, mitem, item, push);

    let drop_def = TraitDef {
        span: span,
        attributes: Vec::new(),
        path: ty::Path::new(vec!("std", "ops", "Drop")),
        additional_bounds: Vec::new(),
        generics: ty::LifetimeBounds::empty(),
        methods: vec![
            MethodDef {
                name: "drop",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: Some(Some(ty::Borrowed(None, Mutability::Mutable))),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![], // todo: handle inlining
                is_unsafe: false,
                combine_substructure: combine_substructure(box drop_substructure),
                unify_fieldless_variants: false,
            }
        ],
        associated_types: vec![],
        is_unsafe: false,
        supports_unions: false, // XXX: Does this trait support unions? I don't know what that would require
    };
    drop_def.expand(cx, mitem, item, push);
}

fn drop_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    cx.expr_if(trait_span,
               cx.expr_call(trait_span, cx.expr_path(cx.path_global(trait_span, vec![
                   cx.ident_of("gc"),
                   cx.ident_of("finalizer_safe"),
               ])), vec![]),
               cx.expr_call(trait_span, cx.expr_path(cx.path_global(trait_span, vec![
                   cx.ident_of("gc"),
                   cx.ident_of("Finalize"),
                   cx.ident_of("finalize"),
               ])), vec![cx.expr_addr_of(trait_span, substr.self_args[0].clone())]),
               None)
}

// Mostly copied from syntax::ext::deriving::hash and Servo's #[jstraceable]
fn trace_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    let trace_path = {
        let strs = vec![
            cx.ident_of("gc"),
            cx.ident_of("Trace"),
            substr.method_ident,
        ];

        cx.expr_path(cx.path_global(trait_span, strs))
    };

    let call_trace = |span, thing_expr| {
        // let expr = cx.expr_method_call(span, thing_expr, trace_ident, vec!());
        let expr = cx.expr_call(span, trace_path.clone(), vec!(cx.expr_addr_of(span, thing_expr)));
        cx.stmt_expr(expr)
    };
    let mut stmts = Vec::new();

    let fields = match *substr.fields {
        Struct(_, ref fs) | EnumMatching(_, _, ref fs) => fs,
        _ => cx.span_bug(trait_span, "impossible substructure in `#[derive(Trace)]`")
    };

    for &FieldInfo { ref self_, span, attrs, .. } in fields.iter() {
        if attrs.iter().all(|ref a| !a.check_name("unsafe_ignore_trace")) {
            stmts.push(call_trace(span, self_.clone()));
        }
    }

    cx.expr_block(cx.block(trait_span, stmts))
}

// Mostly copied from syntax::ext::deriving::hash and Servo's #[jstraceable]
fn finalize_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    let finalize_path = {
        let strs = vec![
            cx.ident_of("gc"),
            cx.ident_of("Finalize"),
            cx.ident_of("finalize"),
        ];
        cx.expr_path(cx.path_global(trait_span, strs))
    };

    let e = trace_substructure(cx, trait_span, substr);
    cx.expr_block(cx.block(trait_span, vec![
        cx.stmt_expr(cx.expr_call(trait_span, finalize_path,
                                  vec![cx.expr_addr_of(trait_span, substr.self_args[0].clone())])),
        cx.stmt_expr(e),
    ]))
}
