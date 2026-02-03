#![deny(warnings)]
#![feature(iterator_try_reduce)]

#[cfg(not(miri))]
use std::time::Instant;
use std::{
    hint,
    sync::atomic::{
        AtomicBool,
        Ordering,
    },
    thread,
    time::Duration,
};

use derive_more::{
    Add,
    Sum,
};
use rstest::rstest;

use ku::{
    error::Error::InvalidArgument,
    log::debug,
    sync::OnceLock,
};

mod log;

#[rstest]
#[timeout(Duration::from_secs(10))]
fn set_get() {
    for _ in 0 .. ITERATION_COUNT {
        let once_lock = OnceLock::new();

        assert_eq!(once_lock.get(), None);
        assert_eq!(once_lock.set(1), Ok(()));
        assert_eq!(once_lock.set(2), Err(InvalidArgument));
        assert!(once_lock.get().is_some());
        assert_eq!(*once_lock.get().unwrap(), 1);
    }
}

#[rstest]
#[timeout(Duration::from_secs(10))]
fn set_get_mut() {
    for _ in 0 .. ITERATION_COUNT {
        let mut once_lock = OnceLock::new();

        assert_eq!(once_lock.get(), None);
        assert_eq!(once_lock.get_mut(), None);
        assert_eq!(once_lock.set(1), Ok(()));
        assert_eq!(once_lock.set(2), Err(InvalidArgument));
        assert!(once_lock.get().is_some());
        assert!(once_lock.get_mut().is_some());
        assert_eq!(*once_lock.get().unwrap(), 1);
        *once_lock.get_mut().unwrap() = 3;
        assert_eq!(*once_lock.get().unwrap(), 3);
    }
}

#[rstest]
#[timeout(Duration::from_secs(60))]
fn single_writer() {
    let available_parallelism = thread::available_parallelism().unwrap();
    let thread_count = available_parallelism.get() * THREAD_COUNT_MULTIPLIER;
    debug!(available_parallelism, thread_count);

    let once_lock = OnceLock::new();
    let run = AtomicBool::new(true);

    let Stats {
        initialized,
        uninitialized,
    } = thread::scope(|scope| {
        let threads: Vec<_> = (1 ..= thread_count)
            .map(|thread| {
                thread::Builder::new()
                    .name(format!("reader #{thread}"))
                    .spawn_scoped(scope, || reader(&once_lock, &run, 1))
                    .unwrap()
            })
            .collect();

        thread::sleep(Duration::from_millis(50));

        once_lock.set([1; ELEMENT_COUNT]).unwrap();

        thread::sleep(Duration::from_millis(50));

        run.store(false, Ordering::Release);

        threads.into_iter().map(|thread| thread.join().unwrap()).sum()
    });

    debug!(initialized, uninitialized);

    assert!(initialized > 0);
    assert!(uninitialized > 0);
}

#[cfg(not(miri))]
#[rstest]
#[timeout(Duration::from_secs(60))]
fn single_writer_is_not_obstructed() {
    const MAX_AVERAGE_OBSTRUCTED_SET_DURATION: Duration = Duration::from_millis(50);
    const OBSTRUCTING_READER_THREAD_COUNT_MULTIPLIER: usize = 10;
    const OBSTRUCTION_ITERATION_COUNT: u32 = 50;

    let available_parallelism = std::thread::available_parallelism().unwrap();
    let obstructing_reader_thread_count =
        available_parallelism.get() * OBSTRUCTING_READER_THREAD_COUNT_MULTIPLIER;
    debug!(available_parallelism, obstructing_reader_thread_count);

    let mut total_set_duration = Duration::default();

    for iteration in 1 ..= OBSTRUCTION_ITERATION_COUNT.try_into().unwrap() {
        let once_lock = OnceLock::new();
        let run = AtomicBool::new(true);

        thread::scope(|scope| {
            let threads: Vec<_> = (1 ..= obstructing_reader_thread_count)
                .map(|thread| {
                    thread::Builder::new()
                        .name(format!("reader #{thread}"))
                        .spawn_scoped(scope, || reader(&once_lock, &run, iteration + 1))
                        .unwrap()
                })
                .collect();

            thread::sleep(Duration::from_millis(50));

            let writer_thread = {
                thread::Builder::new()
                    .name("writer".to_string())
                    .spawn_scoped(scope, || {
                        let start = Instant::now();
                        assert!(writer(&once_lock, &run, iteration));
                        start.elapsed()
                    })
                    .unwrap()
            };

            let set_duration = writer_thread.join().unwrap();

            run.store(false, Ordering::Release);

            let stats = threads.into_iter().map(|thread| thread.join().unwrap()).sum();
            let Stats {
                initialized,
                uninitialized,
            } = stats;

            let initialized_value = once_lock.get().unwrap()[0];
            assert_eq!(initialized_value, iteration);

            debug!(iteration, ?set_duration, initialized, uninitialized);

            total_set_duration += set_duration;
        });
    }

    let average_set_duration = total_set_duration / OBSTRUCTION_ITERATION_COUNT;
    debug!(?average_set_duration);
    assert!(average_set_duration < MAX_AVERAGE_OBSTRUCTED_SET_DURATION);
}

#[rstest]
#[timeout(Duration::from_secs(60))]
fn multiple_writers() {
    let available_parallelism = std::thread::available_parallelism().unwrap();
    let thread_count = available_parallelism.get() * THREAD_COUNT_MULTIPLIER;
    debug!(available_parallelism, thread_count);

    for iteration in 0 .. ITERATION_COUNT {
        let once_lock = OnceLock::new();
        let readers_run = AtomicBool::new(true);
        let writers_run = AtomicBool::new(true);

        thread::scope(|scope| {
            let reader_threads: Vec<_> = (1 ..= thread_count)
                .map(|thread| {
                    thread::Builder::new()
                        .name(format!("reader #{thread}"))
                        .spawn_scoped(scope, || reader(&once_lock, &readers_run, thread_count))
                        .unwrap()
                })
                .collect();

            let writer_threads: Vec<_> = (1 ..= thread_count)
                .map(|thread| {
                    let once_lock = &once_lock;
                    let writers_run = &writers_run;
                    thread::Builder::new()
                        .name(format!("writer #{thread}"))
                        .spawn_scoped(scope, move || writer(once_lock, writers_run, thread))
                        .unwrap()
                })
                .collect();

            thread::sleep(Duration::from_millis(50));

            writers_run.store(true, Ordering::Release);

            let successfully_initialized = writer_threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|&x| x)
                .count();

            thread::sleep(Duration::from_millis(10));

            readers_run.store(false, Ordering::Release);

            let stats = reader_threads.into_iter().map(|thread| thread.join().unwrap()).sum();
            let Stats {
                initialized,
                uninitialized,
            } = stats;

            debug!(
                iteration,
                successfully_initialized, initialized, uninitialized,
            );

            assert_eq!(successfully_initialized, 1);

            assert!(initialized > 0);
        });
    }
}

fn reader(
    once_lock: &OnceLock<[usize; ELEMENT_COUNT]>,
    run: &AtomicBool,
    writer_thread_count: usize,
) -> Stats {
    let mut stats = Stats::default();

    while run.load(Ordering::Acquire) {
        if let Some(data) = once_lock.get() {
            stats.initialized += 1;

            assert!((1 ..= writer_thread_count).contains(&data[0]));
            assert!(
                data.iter()
                    .try_reduce(|x, y| if x == y {
                        Some(x)
                    } else {
                        None
                    })
                    .is_some(),
                "initialized data is inconsistent"
            );
        } else {
            stats.uninitialized += 1;
        }
    }

    stats
}

fn writer(
    once_lock: &OnceLock<[usize; ELEMENT_COUNT]>,
    run: &AtomicBool,
    value: usize,
) -> bool {
    while !run.load(Ordering::Acquire) {
        hint::spin_loop();
    }

    once_lock.set([value; ELEMENT_COUNT]).is_ok()
}

#[derive(Add, Clone, Copy, Default, Sum)]
struct Stats {
    initialized: usize,
    uninitialized: usize,
}

const ELEMENT_COUNT: usize = if cfg!(miri) {
    4
} else {
    10_000
};
const ITERATION_COUNT: usize = if cfg!(miri) {
    10
} else {
    100
};
const THREAD_COUNT_MULTIPLIER: usize = if cfg!(miri) {
    1
} else {
    2
};

#[ctor::ctor]
fn init() {
    log::init();
}
