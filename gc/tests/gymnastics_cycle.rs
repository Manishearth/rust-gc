#![feature(proc_macro, specialization)]

#[macro_use]
extern crate gc_derive;
extern crate gc;

use std::cell::Cell;
use gc::{GcCell, Gc, force_collect};

thread_local!(static COUNTER: Cell<u8> = Cell::new(0u8));

#[derive(Trace)]
struct Cyclic {
    prev: GcCell<Option<Gc<Cyclic>>>,
    name: u8,
}

impl gc::Finalize for Cyclic {
    fn finalize(&self) {
        COUNTER.with(|count| count.set(count.get() + 1));
        println!("Dropped {}", self.name);
    }
}

#[test]
fn test_cycle() {
    {
        let mut gcs = vec![Gc::new(Cyclic {
            prev: GcCell::new(None),
            name: 0,
        })];

        for i in 1..4 {
            let prev = gcs[i-1].clone();
            gcs.push(Gc::new(Cyclic {
                prev: GcCell::new(Some(prev)),
                name: i as u8,
            }));
        }
        let last = gcs[3].clone();
        *gcs[0].prev.borrow_mut() = Some(last);
    }

    println!("Before collection: {:?}", COUNTER.with(|s| s.get()));
    force_collect();
    println!("After collection: {:?}", COUNTER.with(|s| s.get()));
    assert_eq!(COUNTER.with(|s| s.get()), 4);
}
