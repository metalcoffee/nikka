#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::vec::Vec;

use chrono::Duration;

use ku::{
    error::Error::NoDisk,
    time::{
        self,
        TscDuration,
    },
};

use kernel::{
    Subsystems,
    fs::{
        BlockCache,
        Kind,
        test_scaffolding::{
            Bitmap,
            block_cache_init,
            disable_flush,
        },
    },
    log::debug,
};

mod fs_helpers;
mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn reserved_elements() {
    let block = 2;

    block_cache_init(fs_helpers::FS_DISK, block + 1, block + 1).unwrap();

    for start in (0 .. 128).step_by(7) {
        for end in ((start + 1) .. (start + 128)).step_by(13) {
            Bitmap::format(block, start .. end).unwrap();
            let mut bitmap = Bitmap::new(block, start .. end).unwrap();

            for _ in start .. end {
                let element =
                    bitmap.allocate().expect("failed to allocate a supposedly free element");
                assert!(
                    start <= element && element < end,
                    "allocated a reserved element",
                );
            }

            bitmap.allocate().expect_err("allocated from a supposedly empty bitmap");
        }
    }
}

#[test_case]
fn allocation() {
    let (mut block_bitmap, ..) = fs_helpers::simple_fs(Kind::File);

    let free_block_count = (0 .. fs_helpers::BLOCK_COUNT)
        .map(|i| {
            if block_bitmap.is_free(i) {
                1
            } else {
                0
            }
        })
        .sum();
    assert_eq!(
        free_block_count,
        fs_helpers::BLOCK_COUNT - fs_helpers::simple_fs_superblock().blocks().start,
    );

    let mut allocated: Vec<_> =
        (0 .. free_block_count).map(|_| block_bitmap.allocate().unwrap()).collect();
    debug!(
        allocated_head = ?allocated[.. 10],
        allocated_tail = ?allocated[allocated.len() - 10 ..],
    );

    assert_eq!(block_bitmap.allocate(), Err(NoDisk));
    block_bitmap.set_free(fs_helpers::simple_fs_superblock().blocks().start + 5);
    block_bitmap.allocate().unwrap();
    for _ in (1 ..= fs_helpers::BLOCK_COUNT).rev() {
        block_bitmap.set_free(fs_helpers::BLOCK_COUNT - 33);
        block_bitmap.allocate().unwrap();
    }

    introsort::sort(&mut allocated);
    for (i, j) in allocated.iter().zip(&allocated[1 ..]) {
        assert_ne!(i, j, "allocated the same block twice");
    }

    let mut free_block_count = 0;
    for i in allocated.iter().step_by(2) {
        if i % 10_000 == 0 {
            debug!(block = i, free_block_count);
        }
        assert!(!block_bitmap.is_free(*i));
        block_bitmap.set_free(*i);
        free_block_count += 1;
    }

    let mut reallocated: Vec<_> =
        (0 .. free_block_count).map(|_| block_bitmap.allocate().unwrap()).collect();
    debug!(
        reallocated_head = ?reallocated[.. 10],
        reallocated_tail = ?reallocated[reallocated.len() - 10 ..],
    );
    assert_eq!(block_bitmap.allocate(), Err(NoDisk));

    introsort::sort(&mut reallocated);
    for (i, j) in allocated.iter().step_by(2).zip(reallocated) {
        assert_eq!(*i, j);
    }

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn allocation_has_amortized_constant_complexity() {
    disable_flush();

    let bitmap_block = 0;

    for log_total_elements in 2 .. 7 {
        let total_elements = 10_usize.pow(log_total_elements);
        let reserved_elements = total_elements / 20;
        let elements = reserved_elements .. total_elements;

        let bitmap_blocks = Bitmap::size_in_blocks(total_elements);
        block_cache_init(fs_helpers::FS_DISK, bitmap_blocks, bitmap_blocks).unwrap();

        Bitmap::format(bitmap_block, elements.clone()).unwrap();
        let mut bitmap = Bitmap::new(bitmap_block, elements.clone()).unwrap();

        let timer = time::timer();
        for _ in elements.clone() {
            bitmap.allocate().unwrap();
        }
        assert_eq!(bitmap.allocate(), Err(NoDisk));
        let elapsed = timer.elapsed();

        debug!(%elapsed, count = elements.clone().count(), ?elements);

        assert!(
            elapsed < TscDuration::try_from(Duration::seconds(2)).unwrap(),
            "Block::allocation() has wrong time complexity",
        );
    }
}
