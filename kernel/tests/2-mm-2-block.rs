#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::memory::block::Memory;

use kernel::{
    Subsystems,
    log::debug,
    memory::{
        Block,
        Page,
        Virt,
    },
};

mod init;

init!(Subsystems::empty());

#[test_case]
fn block_page() {
    test_block::<Page>();
}

#[test_case]
fn block_virt() {
    test_block::<Virt>();
}

fn test_block<T: Memory>() {
    for start in 0 .. 100 {
        for count in 0 .. 100 {
            let end = start + count;
            let mut block = Block::<T>::from_index(start, end).unwrap();

            if start % 25 == 0 && end % 33 == 0 {
                debug!(start, end, %block);
            }

            assert_eq!(block.count(), count);

            for requested_count in count + 1 .. count + 3 {
                assert!(block.tail(requested_count).is_none());
            }

            let mut requested_count = 0;

            while requested_count < block.count() {
                let old_count = block.count();
                let old_end = block.end();

                block.tail(0).unwrap();
                assert_eq!(block.count(), old_count);

                let b = block.tail(requested_count).unwrap();

                assert_eq!(block.count(), old_count - requested_count);
                assert_eq!(b.count(), requested_count);

                assert_eq!(block.start(), start);
                assert!(block.start() < block.end());
                assert_eq!(block.start() + old_count - requested_count, block.end());
                assert_eq!(block.end(), b.start());
                assert!(b.start() <= b.end());
                assert_eq!(b.start() + requested_count, b.end());
                assert!(b.end() <= end);
                assert_eq!(b.end(), old_end);

                requested_count += 1;
            }

            if count > 0 {
                let remaining_count = block.count();
                assert!(remaining_count > 0);

                let c = block.tail(remaining_count).unwrap();

                assert_eq!(block.count(), 0);
                assert_eq!(c.count(), remaining_count);

                assert_eq!(block.start(), start);
                assert_eq!(block.start(), block.end());
                assert_eq!(block.end(), c.start());
                assert!(c.start() < c.end());
                assert_eq!(c.start() + remaining_count, c.end());
                assert!(c.end() <= end);
            }

            block.tail(0).unwrap();
            assert_eq!(block.count(), 0);
            assert!(block.tail(1).is_none());
        }
    }
}
