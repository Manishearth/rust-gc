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

pub fn expand_trace(cx: &mut ExtCtxt, span: Span, mitem: &MetaItem, item: &Annotatable, push: &mut FnMut(Annotatable)) {
    let trait_def = TraitDef {
        span: span,
        attributes: Vec::new(),
        path: ty::Path::new(vec!("gc", "Trace")),
        additional_bounds: Vec::new(),
        generics: ty::LifetimeBounds::empty(),
        methods: vec![
            MethodDef {
                name: "_trace",
                generics: ty::LifetimeBounds {
                    lifetimes: Vec::new(),
                    bounds: vec![("__T",
                                  vec![ty::Path::new(vec!["gc", "Tracer"])])],
                },
                explicit_self: ty::borrowed_explicit_self(),
                args: vec![ty::Literal(ty::Path::new_local("__T"))],
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure)
            }
        ],
        associated_types: vec![],
    };
    trait_def.expand(cx, mitem, item, push)
}

// Mostly copied from syntax::ext::deriving::hash and Servo's #[jstraceable]
fn trace_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    let tracer_expr = match (substr.nonself_args.len(), substr.nonself_args.get(0)) {
        (1, Some(o_f)) => o_f,
        _ => cx.span_bug(trait_span, "incorrect number of arguments in #[derive(Trace)]")
    };
    let call_traverse = |span, thing_expr| {
        let expr = cx.expr_method_call(span,
                                       tracer_expr.clone(),
                                       cx.ident_of("traverse"),
                                       vec![cx.expr_addr_of(span, thing_expr)]);
        cx.stmt_expr(expr)
    };
    let mut stmts = Vec::new();

    let fields = match *substr.fields {
        Struct(ref fs) | EnumMatching(_, _, ref fs) => fs,
        _ => cx.span_bug(trait_span, "impossible substructure in `#[derive(Trace)]`")
    };

    for &FieldInfo { ref self_, span, .. } in fields {
        stmts.push(call_traverse(span, self_.clone()));
    }

    cx.expr_block(cx.block(trait_span, stmts, None))
}
