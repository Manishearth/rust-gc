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

use syntax::attr::AttrMetaMethods;
use syntax::ext::base::{Annotatable, ExtCtxt, MultiDecorator};
use syntax::codemap::Span;
use syntax::ptr::P;
use syntax::ast::{MetaItem, Expr};
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
                combine_substructure: combine_substructure(box trace_substructure)
            },
            MethodDef {
                name: "root",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure)
            },
            MethodDef {
                name: "unroot",
                generics: ty::LifetimeBounds::empty(),
                explicit_self: ty::borrowed_explicit_self(),
                args: vec!(),
                ret_ty: ty::nil_ty(),
                attributes: vec![],
                is_unsafe: true,
                combine_substructure: combine_substructure(box trace_substructure)
            }
        ],
        associated_types: vec![],
        is_unsafe: true,
    };
    trait_def.expand(cx, mitem, item, push)
}

// Mostly copied from syntax::ext::deriving::hash and Servo's #[jstraceable]
fn trace_substructure(cx: &mut ExtCtxt, trait_span: Span, substr: &Substructure) -> P<Expr> {
    let trace_path = {
        let strs = vec![
            cx.ident_of("gc"),
            cx.ident_of("trace"),
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
        Struct(ref fs) | EnumMatching(_, _, ref fs) => fs,
        _ => cx.span_bug(trait_span, "impossible substructure in `#[derive(Trace)]`")
    };

    for &FieldInfo { ref self_, span, attrs, .. } in fields.iter() {
        if attrs.iter().all(|ref a| !a.check_name("unsafe_ignore_trace")) {
            stmts.push(call_trace(span, self_.clone()));
        }
    }

    cx.expr_block(cx.block(trait_span, stmts, None))
}
