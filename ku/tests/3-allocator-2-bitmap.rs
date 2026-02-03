#![deny(warnings)]

use std::{
    time::{
        Duration,
        Instant,
    },
    vec::Vec,
};

use rstest::rstest;

use ku::{
    collections::Bitmap,
    log::debug,
};

mod log;

#[rstest]
fn basic() {
    const MAX_LEN: usize = 1024;

    for len in 0 ..= MAX_LEN {
        let mut data = [0; MAX_LEN.div_ceil(Bitmap::BITS_PER_ENTRY)];
        let mut bitmap = Bitmap::new(&mut data, len, None);

        assert_eq!(bitmap.free(), len);
        assert_eq!(bitmap.len(), len);
        assert_eq!(bitmap.is_empty(), len == 0);
        assert!(bitmap.validate().is_ok());

        for bit in 0 .. len {
            assert!(bitmap.is_free(bit));
        }

        for free in (0 .. len).rev() {
            assert_eq!(bitmap.free(), free + 1);

            bitmap.allocate().expect("failed to allocate a supposedly free element");

            assert!(bitmap.validate().is_ok());
            assert_eq!(bitmap.free(), free);
        }

        assert_eq!(bitmap.free(), 0);
        assert_eq!(
            bitmap.allocate(),
            None,
            "allocated from a bitmap without free elements",
        );

        for bit in 0 .. len {
            assert!(!bitmap.is_free(bit));
        }

        for bit in 0 .. len {
            assert_eq!(bitmap.free(), bit);

            bitmap.set_free(bit);

            assert!(bitmap.validate().is_ok());
            assert_eq!(bitmap.free(), bit + 1);
        }
    }
}

#[rstest]
fn allocation() {
    const MAX_LEN: usize = 1024;

    for len in 33 ..= MAX_LEN {
        let mut data = [0; MAX_LEN.div_ceil(Bitmap::BITS_PER_ENTRY)];
        let mut bitmap = Bitmap::new(&mut data, len, None);
        let enable_log = len % 33 == 0;

        let mut allocated: Vec<_> = (0 .. len).map(|_| bitmap.allocate().unwrap()).collect();

        if enable_log {
            debug!(
                allocated_head = ?allocated[.. 5],
                allocated_tail = ?allocated[allocated.len() - 5 ..],
            );
        }

        assert_eq!(bitmap.allocate(), None);
        bitmap.set_free(5);
        bitmap.allocate().unwrap();
        for _ in (1 ..= len).rev() {
            bitmap.set_free(len - 33);
            bitmap.allocate().unwrap();
        }

        allocated.sort();
        for (i, j) in allocated.iter().zip(&allocated[1 ..]) {
            assert_ne!(i, j, "allocated the same element twice");
        }

        let mut free = 0;
        for i in allocated.iter().step_by(2) {
            if enable_log && i % 100 == 0 {
                debug!(block = i, free);
            }
            assert!(!bitmap.is_free(*i));
            bitmap.set_free(*i);
            free += 1;
        }

        let mut reallocated: Vec<_> = (0 .. free).map(|_| bitmap.allocate().unwrap()).collect();
        if enable_log {
            debug!(
                reallocated_head = ?reallocated[.. 5],
                reallocated_tail = ?reallocated[reallocated.len() - 5 ..],
            );
        }
        assert_eq!(bitmap.allocate(), None);

        reallocated.sort();
        for (i, j) in allocated.iter().step_by(2).zip(reallocated) {
            assert_eq!(*i, j);
        }
    }
}

#[rstest]
fn allocation_complexity() {
    const BASE: usize = 10;
    const MAX_EXPONENT: u32 = 7;
    const MAX_LEN: usize = BASE.pow(MAX_EXPONENT);

    let mut time_by_log_len = Vec::<Duration>::new();

    for log_len in 2 ..= MAX_EXPONENT {
        let len = BASE.pow(log_len);

        let mut data = [0; MAX_LEN.div_ceil(Bitmap::BITS_PER_ENTRY)];
        let mut bitmap = Bitmap::new(&mut data, len, None);

        let timer = Instant::now();

        for _ in 0 .. len {
            bitmap.allocate().unwrap();
        }
        assert_eq!(bitmap.allocate(), None);
        let elapsed = timer.elapsed();

        let amortized_allocation_time = elapsed / len.try_into().unwrap();
        debug!(len, ?elapsed, ?amortized_allocation_time);
        time_by_log_len.push(amortized_allocation_time);

        assert!(
            elapsed < Duration::from_secs(2),
            "Bitmap::allocate() is too slow",
        );
    }

    let avg_time =
        time_by_log_len.iter().sum::<Duration>() / time_by_log_len.len().try_into().unwrap();
    let min_time = *time_by_log_len.iter().min().unwrap();
    let max_time = *time_by_log_len.iter().max().unwrap();
    debug!(?min_time, ?avg_time, ?max_time);
    assert!(
        avg_time / 3 < min_time && max_time < 3 * avg_time,
        "Bitmap::allocate() should have constant amortized time complexity",
    );
}

#[ctor::ctor]
fn init() {
    log::init();
}
