#![feature(plugin_registrar, box_syntax)]
#![feature(rustc_private)]

#[macro_use]
extern crate syntax;
#[macro_use]
extern crate rustc;



use rustc::plugin::Registry;
use syntax::parse::token::intern;

use syntax::ext::base::{Annotatable, ExtCtxt, MultiDecorator};
use syntax::codemap::Span;
use syntax::ptr::P;
use syntax::ast::{MetaItem, Expr};
use syntax::ext::build::AstBuilder;
use syntax::ext::deriving::generic::{combine_substructure, EnumMatching, FieldInfo, MethodDef, Struct, Substructure, TraitDef, ty};


#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
    reg.register_syntax_extension(intern("derive_Trace"), MultiDecorator(box expand_trace))
}

pub fn expand_trace(cx: &mut ExtCtxt, span: Span, mitem: &MetaItem, item: Annotatable, push: &mut FnMut(Annotatable)) {
    let trait_def = TraitDef {
        span: span,
        attributes: Vec::new(),
        path: ty::Path::new(vec!("gc", "trace", "Trace")),
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
                combine_substructure: combine_substructure(box trace_substructure)
            },
            MethodDef {
                name: "root",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                combine_substructure: combine_substructure(box trace_substructure)
            },
            MethodDef {
                name: "unroot",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                combine_substructure: combine_substructure(box trace_substructure)
            }
        ],
        associated_types: vec![],
    };
    trait_def.expand(cx, mitem, &item, push)
}

// Mostly copied from syntax::ext::deriving::hash and Servo's #[jstraceable]
fn trace_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    let trace_ident = substr.method_ident;
    let call_trace = |span, thing_expr| {
        let expr = cx.expr_method_call(span, thing_expr, trace_ident, vec!());
        cx.stmt_expr(expr)
    };
    let mut stmts = Vec::new();

    let fields = match *substr.fields {
        Struct(ref fs) | EnumMatching(_, _, ref fs) => fs,
        _ => cx.span_bug(trait_span, "impossible substructure in `#[derive(Trace)]`")
    };

    for &FieldInfo { ref self_, span, .. } in fields.iter() {
        stmts.push(call_trace(span, self_.clone()));
    }

    cx.expr_block(cx.block(trait_span, stmts, None))
}
