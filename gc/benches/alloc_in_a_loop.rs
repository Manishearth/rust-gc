use criterion::{Criterion, criterion_group, criterion_main};

const THING: u64 = 0;

fn discard(c: &mut Criterion, n: usize) {
    c.bench_function(&format!("discard_{}", n), |b| {
        b.iter(|| {
            gc::force_collect();
            for _ in 0..n {
                std::hint::black_box(gc::Gc::new(THING));
            }
        })
    });
}

fn keep(c: &mut Criterion, n: usize) {
    c.bench_function(&format!("keep_{}", n), |b| {
        b.iter(|| {
            gc::force_collect();
            (0..n).map(|_| gc::Gc::new(THING)).collect::<Vec<_>>()
        })
    });
}

fn benches(c: &mut Criterion) {
    discard(c, 100);
    keep(c, 100);
    discard(c, 10_000);
    keep(c, 10_000);
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
