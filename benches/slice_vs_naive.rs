//! This benchmark test is designed to answer the following question: How many
//! elements have to be locked simultaneously for the interval locks to pay
//! off?
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::{convert::TryInto, pin::Pin};

use interlock::hl::slice::SyncRbTreeSliceIntervalRwLock;

/// The number of subslices to borrow with the interval locks
const NUM_BORROWS: usize = 16;

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Locking array elements");
    for i in (5..=19).step_by(2) {
        let num_elems = 1usize << i;
        group.throughput(Throughput::Elements(num_elems.try_into().unwrap()));

        macro_rules! bench_naive {
            ($name:expr, $ctor:expr, |$lock_var:ident| $lock:expr) => {
                group.bench_function(
                    BenchmarkId::new(
                        format!("{} per element, lock-unlock the whole", $name),
                        num_elems,
                    ),
                    |b| {
                        let vec: Vec<_> = (0..num_elems).map(|_| $ctor).collect();
                        b.iter(|| {
                            let mut guards: Vec<_> = vec.iter().map(|$lock_var| $lock).collect();
                            for guard in guards.iter_mut() {
                                touch_elem(&mut *guard);
                            }
                            while guards.pop().is_some() {}
                        });
                    },
                );

                group.bench_function(
                    BenchmarkId::new(
                        format!("{} per element, lock-unlock each", $name),
                        num_elems,
                    ),
                    |b| {
                        let vec: Vec<_> = (0..num_elems).map(|_| $ctor).collect();
                        b.iter(|| {
                            for $lock_var in vec.iter() {
                                let mut guard = $lock;
                                touch_elem(&mut *guard);
                            }
                        });
                    },
                );
            };
        }

        bench_naive!("std::sync::Mutex", std::sync::Mutex::new(0), |m| m
            .lock()
            .unwrap());
        bench_naive!("std::sync::RwLock", std::sync::RwLock::new(0), |m| m
            .write()
            .unwrap());
        bench_naive!("parking_lot::Mutex", parking_lot::Mutex::new(0), |m| m
            .lock());
        bench_naive!("parking_lot::RwLock", parking_lot::RwLock::new(0), |m| m
            .write());

        // The ranges to lock (the whole array divided into `NUM_BORROWS` pieces)
        let lock_points: Vec<_> = (0..=NUM_BORROWS)
            .map(|i| i * num_elems / NUM_BORROWS)
            .collect();
        let lock_ranges = lock_points.windows(2).map(|e| e[0]..e[1]);

        group.bench_function(
            BenchmarkId::new(
                "SyncRbTreeSliceIntervalRwLock, lock-unlock the whole",
                num_elems,
            ),
            |b| {
                let rwl = Box::pin(SyncRbTreeSliceIntervalRwLock::new(vec![0; num_elems]));
                let rwl = rwl.as_ref();
                let mut state: Vec<_> = (0..NUM_BORROWS)
                    .map(|_| Box::pin(Default::default()))
                    .collect();
                b.iter(|| {
                    let mut guards: Vec<_> = state
                        .iter_mut()
                        .zip(lock_ranges.clone())
                        .map(|(state, lock_range)| rwl.write(lock_range, (), Pin::as_mut(state)))
                        .collect();

                    for guard in guards.iter_mut() {
                        touch_slice(&mut *guard);
                    }

                    while guards.pop().is_some() {}
                });
            },
        );

        group.bench_function(
            BenchmarkId::new(
                "SyncRbTreeSliceIntervalRwLock, lock-unlock each block",
                num_elems,
            ),
            |b| {
                let rwl = Box::pin(SyncRbTreeSliceIntervalRwLock::new(vec![0; num_elems]));
                let rwl = rwl.as_ref();
                interlock::state!(let mut state);
                b.iter(|| {
                    for lock_range in lock_ranges.clone() {
                        let mut guard = rwl.write(lock_range, (), Pin::as_mut(&mut state));
                        touch_slice(&mut guard);
                    }
                });
            },
        );
    }
}

#[inline(never)]
fn touch_elem(x: &mut u32) {
    *x = x.wrapping_add(1);
}

#[inline(never)]
fn touch_slice(x: &mut [u32]) {
    for e in x.iter_mut() {
        *e = e.wrapping_add(1);
    }
}
