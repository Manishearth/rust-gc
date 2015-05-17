#![feature(plugin)]
#![plugin(gc_plugin)]
#![feature(custom_derive)]

extern crate gc;

#[derive(Trace)]
struct Foo {
    y: u8, //~ ERROR no method named
    x: u8 //~ ERROR no method named
}

fn main(){}