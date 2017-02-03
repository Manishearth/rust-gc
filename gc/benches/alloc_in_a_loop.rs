#![feature(test)]

extern crate test;
extern crate gc;

const THING: u64 = 0;

fn discard(b: &mut test::Bencher, n: usize) {
    b.iter(|| {
        gc::force_collect();
        for _ in 0..n {
            test::black_box(gc::Gc::new(THING));
        }
    })
}
fn keep(b: &mut test::Bencher, n: usize) {
    b.iter(|| {
        gc::force_collect();
        (0..n)
            .map(|_| gc::Gc::new(THING))
            .collect::<Vec<_>>()
    })
}

#[bench]
fn discard_100(b: &mut test::Bencher) {
    discard(b, 100)
}
#[bench]
fn keep_100(b: &mut test::Bencher) {
    keep(b, 100)
}
#[bench]
fn discard_10000(b: &mut test::Bencher) {
    discard(b, 10_000)
}
#[bench]
fn keep_10000(b: &mut test::Bencher) {
    keep(b, 10_000)
}
