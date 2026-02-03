#![deny(warnings)]

use std::{
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
    log::{
        debug,
        error,
    },
    sync::SequenceLock,
};

mod log;

#[rstest]
#[timeout(Duration::from_secs(1))]
fn write() {
    let sequence_lock = SequenceLock::new(0);

    unsafe {
        sequence_lock.write().set(3);
    }

    let a = sequence_lock.read();
    let b = sequence_lock.read();

    assert_eq!(a, 3);
    assert_eq!(b, 3);
}

#[rstest]
#[timeout(Duration::from_secs(1))]
fn write_lock() {
    let sequence_lock = SequenceLock::new(0);

    sequence_lock.write_lock().set(3);

    let a = sequence_lock.read();
    let b = sequence_lock.read();

    assert_eq!(a, 3);
    assert_eq!(b, 3);
}

#[rstest]
#[timeout(Duration::from_secs(1))]
fn exclusive_access() {
    let mut sequence_lock = SequenceLock::new(0);

    *sequence_lock.get_mut() = 3;

    let a = sequence_lock.read();
    let b = sequence_lock.read();

    assert_eq!(a, 3);
    assert_eq!(b, 3);
}

#[rstest]
#[timeout(Duration::from_secs(60))]
fn single_writer() {
    let run = AtomicBool::new(true);
    let sequence_lock = SequenceLock::new((0, 0));

    let Stats {
        consistent,
        inconsistent,
    } = thread::scope(|scope| {
        let reader_threads: Vec<_> = (0 .. THREAD_COUNT)
            .map(|thread| {
                thread::Builder::new()
                    .name(format!("reader #{thread}"))
                    .spawn_scoped(scope, || reader(&sequence_lock))
                    .unwrap()
            })
            .collect();

        let writer_thread = {
            thread::Builder::new()
                .name("writer".to_string())
                .spawn_scoped(scope, || exclusive_writer(&sequence_lock, &run))
                .unwrap()
        };

        let stats = reader_threads
            .into_iter()
            .map(|thread| thread.join().expect("readers should finish successfully"))
            .sum();

        run.store(false, Ordering::Release);

        writer_thread.join().expect("writer should finish successfully");

        stats
    });

    debug!(consistent, inconsistent);
    assert!(
        consistent >= THREAD_COUNT * MIN_DIFFERENT_READS,
        "detected only {consistent} consistent data reads",
    );
    assert_eq!(
        inconsistent, 0,
        "detected {inconsistent} inconsistent data reads",
    );
}

#[rstest]
#[timeout(Duration::from_secs(300))]
fn multiple_writers() {
    let run = AtomicBool::new(true);
    let sequence_lock = SequenceLock::new((0, 0));

    let Stats {
        consistent,
        inconsistent,
    } = thread::scope(|scope| {
        let reader_threads: Vec<_> = (0 .. THREAD_COUNT)
            .map(|thread| {
                thread::Builder::new()
                    .name(format!("reader #{thread}"))
                    .spawn_scoped(scope, || reader(&sequence_lock))
                    .unwrap()
            })
            .collect();

        let writer_threads: Vec<_> = (0 .. THREAD_COUNT)
            .map(|thread| {
                thread::Builder::new()
                    .name(format!("writer #{thread}"))
                    .spawn_scoped(scope, || non_exclusive_writer(&sequence_lock, &run))
                    .unwrap()
            })
            .collect();

        let stats = reader_threads
            .into_iter()
            .map(|thread| thread.join().expect("readers should finish successfully"))
            .sum();

        run.store(false, Ordering::Release);

        let writers_finished_successfully = writer_threads
            .into_iter()
            .map(|thread| thread.join())
            .all(|result| result.is_ok());
        assert!(writers_finished_successfully);

        stats
    });

    debug!(consistent, inconsistent);
    assert!(
        consistent >= THREAD_COUNT * MIN_DIFFERENT_READS,
        "detected only {consistent} consistent data reads",
    );
    assert_eq!(
        inconsistent, 0,
        "detected {inconsistent} inconsistent data reads",
    );
}

#[rstest]
#[cfg_attr(not(miri), timeout(Duration::from_secs(60)))]
fn multiple_exclusive_writers() {
    let run = AtomicBool::new(true);
    let sequence_lock = SequenceLock::new((0, 0));

    thread::scope(|scope| {
        let writer_threads: Vec<_> = (0 .. THREAD_COUNT)
            .map(|thread| {
                thread::Builder::new()
                    .name(format!("writer #{thread}"))
                    .spawn_scoped(scope, || exclusive_writer(&sequence_lock, &run))
                    .unwrap()
            })
            .collect();

        thread::sleep(Duration::from_secs(1));

        run.store(false, Ordering::Release);

        let non_exclusivity_detection_count = writer_threads
            .into_iter()
            .map(|thread| thread.join())
            .filter(|result| result.is_err())
            .count();

        debug!(non_exclusivity_detection_count);
        assert!(non_exclusivity_detection_count > 0);
    });
}

fn reader(sequence_lock: &SequenceLock<(usize, usize)>) -> Stats {
    let mut iteration = 0;
    let mut prev = 0;
    let mut report = true;
    let mut stats = Stats::default();

    while iteration < MIN_DIFFERENT_READS {
        let data = sequence_lock.read();

        if 2 * data.0 == data.1 {
            if data.0 > 0 {
                stats.consistent += 1;
            }
        } else {
            if report {
                error!(?data, iteration, "inconsistent data");
                report = false;
            }
            stats.inconsistent += 1;
        }

        if data.0 != prev {
            prev = data.0;
            iteration += 1;
        }
    }

    stats
}

fn exclusive_writer(
    sequence_lock: &SequenceLock<(usize, usize)>,
    run: &AtomicBool,
) {
    while run.load(Ordering::Acquire) {
        let mut lock = unsafe { sequence_lock.write() };
        let data = lock.get();

        let i = data.0;
        lock.set((i + 1, 0));

        thread::yield_now();

        lock.set((i + 1, 2 * i + 2));
    }
}

fn non_exclusive_writer(
    sequence_lock: &SequenceLock<(usize, usize)>,
    run: &AtomicBool,
) {
    while run.load(Ordering::Acquire) {
        let mut lock = sequence_lock.write_lock();
        let data = lock.get();

        let i = data.0;
        lock.set((i + 1, 0));

        thread::yield_now();

        lock.set((i + 1, 2 * i + 2));
    }
}

#[derive(Add, Clone, Copy, Default, Sum)]
struct Stats {
    consistent: usize,
    inconsistent: usize,
}

const MIN_DIFFERENT_READS: usize = if cfg!(miri) {
    10
} else {
    50
};
const THREAD_COUNT: usize = 5;

#[ctor::ctor]
fn init() {
    log::init();
}
