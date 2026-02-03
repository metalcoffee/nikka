#![deny(warnings)]
#![feature(allocator_api)]

use std::{
    rc::Rc,
    sync::Mutex,
    time::{
        Duration,
        Instant,
    },
    vec::Vec,
};

use rand::{
    Rng,
    SeedableRng,
    rngs::SmallRng,
    seq::SliceRandom,
};
use rstest::rstest;

use ku::{
    allocator::test_scaffolding::{
        Quarry,
        SLAB_SIZE,
    },
    log::debug,
    memory::{
        Page,
        Virt,
    },
};

use allocator::Fallback;

mod allocator;
mod log;

#[rstest]
fn stress() {
    let allocator = Fallback::new();
    let map_count = 10;
    let mut log_time = Instant::now();
    let mut rng = SmallRng::seed_from_u64(SEED);

    for size in (1 ..= SLAB_SIZE).filter(|&x| {
        x <= 128 ||
            x <= SLAB_SIZE / 16 && (x + 3) % (Page::SIZE / 4) < 7 ||
            (x + 3) % Page::SIZE < 7
    }) {
        if log_time < Instant::now() {
            debug!(size);
            log_time += Duration::from_secs(1);
        }

        let mut quarry_on_the_heap = Rc::new(Quarry::new(size));
        let quarry = Rc::<Quarry>::get_mut(&mut quarry_on_the_heap).unwrap();
        let mut bulk_frame_count = 0;
        let mut allocation_count = 0;

        let mut allocation_counts: Vec<_> = (3 .. quarry.capacity() - 2)
            .filter(|x| (x + 3) * size % Page::SIZE < 7 * size)
            .collect();
        if allocation_counts.len() > map_count {
            allocation_counts =
                allocation_counts.choose_multiple(&mut rng, map_count).cloned().collect();
        }
        if rng.gen_ratio(1, 3) {
            allocation_counts.push(1);
        } else if rng.gen_ratio(1, 3) {
            allocation_counts.push(2);
        } else {
            allocation_counts.push(3);
        }
        if rng.gen_ratio(1, 2) {
            allocation_counts.push(quarry.capacity() - 2);
        } else {
            allocation_counts.push(quarry.capacity() - 1);
        }
        allocation_counts.push(quarry.capacity());
        allocation_counts.sort();
        let mut allocation_counts = allocation_counts.iter();

        while allocation_count < quarry.capacity() {
            let requested_allocation_count =
                *(allocation_counts.find(|&&x| x > allocation_count).unwrap());
            let mut new_allocation_count = requested_allocation_count;
            bulk_frame_count +=
                quarry.map(allocation_count, &mut new_allocation_count, &allocator).unwrap();
            allocation_count = new_allocation_count;

            assert_ne!(quarry.allocation(allocation_count - 1), Virt::default());

            let mut allocations = Vec::new();

            for allocation_index in (0 .. new_allocation_count)
                .filter(|x| (x + 3) * size % Page::SIZE < 7 * size)
                .filter(|x| {
                    let page = x * size / Page::SIZE;
                    (page + 3) % 31 < 7
                })
            {
                let allocation = quarry.allocation(allocation_index);
                assert_ne!(allocation, Virt::default());

                allocations.push(allocation);

                unsafe {
                    allocation.try_into_mut_slice::<u8>(size).unwrap().fill(13);
                }

                assert_eq!(
                    quarry.allocation_index(allocation).unwrap(),
                    allocation_index,
                );
                assert_eq!(quarry.allocation_index(Virt::from_ref(quarry)), None);
            }

            allocations.sort();
            for (&a, &b) in allocations.iter().zip(allocations.iter().skip(1)) {
                assert!((b - a).unwrap() >= size);
            }
        }

        assert_eq!(allocation_count, quarry.capacity());
        assert_eq!(
            quarry.unmap(allocation_count, &allocator).unwrap(),
            bulk_frame_count,
        );
    }
}

#[rstest]
#[should_panic]
fn out_of_bounds() {
    let mut quarry = Quarry::new(2);
    quarry.allocation(0);
}

#[rstest]
#[should_panic]
fn wrong_alignment() {
    static QUARRY: Mutex<Quarry> = Mutex::new(Quarry::new(2));

    let allocator = Fallback::new();
    let mut new_allocation_count = 16;
    let mut quarry = QUARRY.lock().unwrap();
    let map_result = quarry.map(0, &mut new_allocation_count, &allocator);

    if map_result.is_ok() {
        let allocation = quarry.allocation(0);

        if let Ok(wrong_addr) = allocation + 1 {
            quarry.allocation_index(wrong_addr);
        }
    }
}

#[ctor::ctor]
fn init() {
    log::init();
}

const SEED: u64 = 314159265;
