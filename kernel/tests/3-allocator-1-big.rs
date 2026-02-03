#![deny(warnings)]
#![feature(allocator_api)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![feature(maybe_uninit_fill, maybe_uninit_slice)]
#![feature(ptr_as_uninit)]
#![feature(slice_ptr_get)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::{
    alloc::{
        Allocator,
        Global,
        Layout,
    },
    boxed::Box,
    collections::BTreeMap,
    vec::Vec,
};
use core::{
    cmp,
    mem::{
        self,
        MaybeUninit,
    },
};

use static_assertions::const_assert;

use ku::{
    allocator::{
        BigAllocator,
        DryAllocator,
        Info,
        Initialize,
    },
    error::{
        Error::{
            InvalidArgument,
            NoPage,
            PermissionDenied,
        },
        Result,
    },
    memory::{
        KERNEL_RW,
        block::Memory,
    },
};

use kernel::{
    Subsystems,
    allocator,
    log::debug,
    memory::{
        AddressSpace,
        BASE_ADDRESS_SPACE,
        Block,
        FRAME_ALLOCATOR,
        Page,
        Virt,
        mmu::{
            PageTableEntry,
            PageTableFlags,
            USER_R,
            USER_RW,
        },
        size::{
            KiB,
            MiB,
            Size,
        },
        test_scaffolding::{
            PAGES_PER_ROOT_LEVEL_ENTRY,
            translate,
            user_root_level_entries,
        },
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::MEMORY);

#[test_case]
fn basic() {
    memory_allocator_basic();
}

#[test_case]
fn alignment() {
    memory_allocator_alignment();
}

#[test_case]
fn grow_and_shrink_unmap_old_block() {
    for initialize in [Initialize::Garbage, Initialize::Zero] {
        grow_and_shrink_unmap_old_block_and_initialize_memory(initialize);
    }
}

fn grow_and_shrink_unmap_old_block_and_initialize_memory(initialize: Initialize) {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    debug!(?initialize);

    let small_layout = Page::layout_array(4);
    let small_allocation =
        address_space.allocator(USER_RW).dry_allocate(small_layout, initialize).unwrap();
    let small_block = Block::from_slice(unsafe { small_allocation.as_uninit_slice() }).enclosing();
    validate_initialization(small_block, initialize);

    let big_layout = Page::layout_array(8);
    let big_allocation = unsafe {
        address_space
            .allocator(USER_RW)
            .dry_grow(
                small_allocation.as_non_null_ptr(),
                small_layout,
                big_layout,
                initialize,
            )
            .unwrap()
    };
    let big_block = Block::from_slice(unsafe { big_allocation.as_uninit_slice() }).enclosing();
    validate_initialization(big_block, initialize);

    debug!(%small_block, %big_block);

    let unmapped_pages = difference_should_be_unmapped(&mut address_space, small_block, big_block);
    debug!(unmapped_pages);
    assert!(unmapped_pages <= small_block.count());

    let medium_layout = Page::layout_array(6);
    let medium_allocation = unsafe {
        address_space
            .allocator(USER_RW)
            .dry_shrink(big_allocation.as_non_null_ptr(), big_layout, medium_layout)
            .unwrap()
    };
    let medium_block =
        Block::from_slice(unsafe { medium_allocation.as_uninit_slice() }).enclosing();
    validate_initialization(medium_block, initialize);

    debug!(%big_block, %medium_block);

    let unmapped_pages = difference_should_be_unmapped(&mut address_space, big_block, medium_block);
    debug!(unmapped_pages);
    assert!(unmapped_pages >= big_block.count() - medium_block.count());

    unsafe {
        address_space
            .allocator(USER_RW)
            .dry_deallocate(medium_allocation.as_non_null_ptr(), medium_layout);
    }

    fn difference_should_be_unmapped(
        address_space: &mut AddressSpace,
        old: Block<Page>,
        new: Block<Page>,
    ) -> usize {
        let old_minus_new_left =
            Block::<Page>::from_index(old.start(), cmp::min(old.end(), new.start()));
        let old_minus_new_right =
            Block::<Page>::from_index(cmp::max(old.start(), new.end()), old.end());

        let mut unmapped_pages = 0;

        for block in [old_minus_new_left, old_minus_new_right].into_iter().flatten() {
            for page in block {
                assert_eq!(unsafe { address_space.unmap_page(page) }, Err(NoPage));
            }
            unmapped_pages += block.count();
        }

        unmapped_pages
    }
}

#[test_case]
fn shrink_is_not_a_noop() {
    for initialize in [Initialize::Garbage, Initialize::Zero] {
        shrink_is_not_a_noop_and_initializes_memory(initialize);
    }
}

fn shrink_is_not_a_noop_and_initializes_memory(initialize: Initialize) {
    let mut address_space = BASE_ADDRESS_SPACE.lock();

    debug!(?initialize);

    let old_layout = Page::layout_array(4);
    let old_allocation =
        address_space.allocator(USER_RW).dry_allocate(old_layout, initialize).unwrap();
    let old_block = Block::from_slice(unsafe { old_allocation.as_uninit_slice() });
    validate_initialization(old_block, initialize);

    let new_layout = Page::layout_array(3);
    let new_allocation = unsafe {
        address_space
            .allocator(USER_RW)
            .dry_shrink(old_allocation.as_non_null_ptr(), old_layout, new_layout)
            .unwrap()
    };
    let new_block = Block::from_slice(unsafe { new_allocation.as_uninit_slice() });
    validate_initialization(new_block, initialize);

    assert_ne!(old_block.count(), new_block.count());

    debug!(%old_block, %new_block, no_remap_on_shrink = old_block.contains_block(new_block));

    unsafe {
        address_space
            .allocator(USER_RW)
            .dry_deallocate(new_allocation.as_non_null_ptr(), new_layout);
    }
}

#[test_case]
fn copy_mapping_frame_references() {
    let mut address_space = BASE_ADDRESS_SPACE.lock();
    let layout = Page::layout_array(3);

    let mut old_block = Block::default();
    let mut new_block = Block::default();

    for block in [&mut old_block, &mut new_block] {
        *block = address_space.allocator(USER_RW).reserve(layout).unwrap();
        check_mapped(&mut address_space, *block, false);
    }

    unsafe {
        address_space.allocator(USER_RW).map(old_block, USER_RW).unwrap();
    }

    check_mapped(&mut address_space, old_block, true);
    check_mapped(&mut address_space, new_block, false);
    check_reference_count(&mut address_space, old_block, 1);

    unsafe {
        address_space
            .allocator(USER_RW)
            .copy_mapping(old_block, new_block, None)
            .unwrap();
    }

    check_mapped(&mut address_space, old_block, true);
    check_equal(&mut address_space, old_block, new_block);
    check_reference_count(&mut address_space, old_block, 2);

    unsafe {
        address_space.allocator(USER_RW).unmap(old_block).unwrap();
    }

    check_mapped(&mut address_space, old_block, false);
    check_mapped(&mut address_space, new_block, true);
    check_reference_count(&mut address_space, new_block, 1);

    unsafe {
        address_space.allocator(USER_RW).unmap(new_block).unwrap();
    }

    for block in [old_block, new_block] {
        check_mapped(&mut address_space, block, false);

        unsafe {
            address_space.allocator(USER_RW).unreserve(block).unwrap();
        }
    }

    fn check_equal(
        address_space: &mut AddressSpace,
        a: Block<Page>,
        b: Block<Page>,
    ) {
        debug!(%a, %b, "blocks should be equal");

        for (a_page, b_page) in a.into_iter().zip(b) {
            let a_pte = translate(address_space, a_page.address()).map(|pte| *pte);
            let b_pte = translate(address_space, b_page.address()).map(|pte| *pte);
            debug!(?a_pte, ?b_pte);
            assert_eq!(a_pte, b_pte);
        }
    }

    fn check_reference_count(
        address_space: &mut AddressSpace,
        block: Block<Page>,
        expected_reference_count: usize,
    ) {
        debug!(%block, expected_reference_count);

        for page in block {
            let translate_result = translate(address_space, page.address());
            if let Ok(pte) = translate_result {
                let reference_count =
                    FRAME_ALLOCATOR.lock().reference_count(pte.frame().unwrap()).unwrap();
                debug!(%page, ?pte, reference_count);
                assert_eq!(reference_count, expected_reference_count);
            } else {
                debug!(%page, ?translate_result);
                assert_eq!(
                    expected_reference_count, 0,
                    "intermediate page is not mapped?",
                );
            }
        }
    }

    fn check_mapped(
        address_space: &mut AddressSpace,
        block: Block<Page>,
        should_be_mapped: bool,
    ) {
        debug!(%block, should_be_mapped);

        for page in block {
            let translate_result = translate(address_space, page.address());
            if let Ok(pte) = translate_result {
                debug!(%page, ?pte);
                assert_eq!(pte.is_present(), should_be_mapped, "pte: {pte:?}");
            } else {
                debug!(%page, ?translate_result);
                assert!(!should_be_mapped, "intermediate page is not mapped?");
            }
        }
    }
}

#[test_case]
fn copy_mapping_same_block() {
    let mut address_space = BASE_ADDRESS_SPACE.lock();
    let layout = Page::layout_array(3);

    let block = address_space.allocator(USER_RW).reserve(layout).unwrap();

    unsafe {
        address_space.allocator(USER_RW).map(block, USER_RW).unwrap();
    }

    let value = 42;
    unsafe {
        block.try_into_mut_slice::<MaybeUninit<usize>>().unwrap().write_filled(value);
    }

    unsafe {
        address_space
            .allocator(USER_RW)
            .copy_mapping(block, block, Some(USER_R))
            .unwrap();
    }

    let pte = translate(
        &mut address_space,
        block.into_iter().next().unwrap().address(),
    )
    .unwrap();
    assert!(!pte.is_writable());

    let slice =
        unsafe { block.try_into_mut_slice::<MaybeUninit<usize>>().unwrap().assume_init_mut() };

    assert!(slice.iter().all(|&x| x == value));
}

#[test_case]
fn copy_mapping_corner_cases() {
    let start = user_root_level_entries().start * PAGES_PER_ROOT_LEVEL_ENTRY;

    let big_1 = Block::from_index(start + 5, start + 10).unwrap();
    let big_2 = Block::from_index(start + 8, start + 13).unwrap();
    let small_1 = Block::from_index(start, start + 1).unwrap();
    let small_2 = Block::from_index(start + 6, start + 7).unwrap();

    for block in [small_1, small_2, big_1, big_2] {
        let mut address_space = BASE_ADDRESS_SPACE.lock();
        unsafe {
            address_space.allocator(USER_RW).map(block, USER_RW).unwrap();
        }
    }

    for (old_block, new_block) in [
        (big_1, big_2),
        (big_1, small_1),
        (big_1, small_2),
        (small_1, big_1),
        (small_2, big_1),
    ] {
        for flags in [None, Some(USER_R), Some(KERNEL_RW)] {
            validate_copy_mapping(old_block, new_block, flags, Err(InvalidArgument));
        }
    }

    for (flags, expected) in [
        (None, Err(InvalidArgument)),
        (Some(USER_R), Ok(())),
        (Some(KERNEL_RW), Err(PermissionDenied)),
    ] {
        validate_copy_mapping(small_1, small_1, flags, expected);
    }

    for (flags, expected) in [
        (None, Ok(())),
        (Some(USER_R), Ok(())),
        (Some(KERNEL_RW), Err(PermissionDenied)),
    ] {
        validate_copy_mapping(small_1, small_2, flags, expected);
    }

    fn validate_copy_mapping(
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
        expected_result: Result<()>,
    ) {
        let blocks_message = if old_block == new_block {
            "the same block"
        } else if old_block.size() == new_block.size() {
            if old_block.is_disjoint(new_block) {
                "disjoint blocks of the same size"
            } else {
                "different intersecting blocks of the same size"
            }
        } else if old_block.contains_block(new_block) {
            "old block is a strict superblock"
        } else if new_block.contains_block(old_block) {
            "new block is a strict superblock"
        } else {
            "block size differs"
        };

        let flags_message = if let Some(flags) = flags {
            if flags.is_user() {
                "allowed"
            } else {
                "disallowed"
            }
        } else {
            "no"
        };

        debug!(
            ?expected_result,
            %old_block,
            %new_block,
            ?flags,
            "{}, {} flags",
            blocks_message,
            flags_message,
        );

        let actual_result = unsafe {
            BASE_ADDRESS_SPACE
                .lock()
                .allocator(USER_RW)
                .copy_mapping(old_block, new_block, flags)
        };

        assert_eq!(actual_result, expected_result);
    }
}

#[test_case]
fn grow_and_shrink() {
    memory_allocator_grow_and_shrink();
}

#[test_case]
fn paged_realloc_is_cheap() {
    #[repr(C, align(4096))]
    pub struct PagedType(u8);

    const_assert!(mem::align_of::<PagedType>() == Page::SIZE);
    const_assert!(mem::size_of::<PagedType>() == Page::SIZE);

    let mut vec = Vec::new();
    let mut prev_frames = Vec::new();

    for _ in 0 .. 5 {
        vec.push(PagedType(0));
        while vec.len() < vec.capacity() {
            vec.push(PagedType(0));
        }

        prev_frames = check_frames(&vec, prev_frames);
    }

    while !vec.is_empty() {
        vec.pop().unwrap();
        vec.shrink_to_fit();

        prev_frames = check_frames(&vec, prev_frames);
    }

    assert!(prev_frames.is_empty());
}

#[test_case]
fn stress() {
    let values = 10_000;
    let max_fragmentation_loss = |values| cmp::max(8 * KiB * values, 16 * MiB);
    memory_allocator_stress(values, max_fragmentation_loss);
}

fn check_frames<T>(
    vec: &Vec<T>,
    prev_frames: Vec<PageTableEntry>,
) -> Vec<PageTableEntry> {
    let block = Block::from_slice(vec.as_slice()).enclosing();
    let mut frames = Vec::with_capacity(block.count());
    for page in block {
        let mut pte = *translate(&mut BASE_ADDRESS_SPACE.lock(), page.address()).unwrap();
        pte.set_flags(pte.flags() & !(PageTableFlags::ACCESSED | PageTableFlags::DIRTY));
        frames.push(pte)
    }
    debug!(%block, ?frames);

    for pte in &frames {
        let reference_count = FRAME_ALLOCATOR.lock().reference_count(pte.frame().unwrap()).unwrap();
        assert_eq!(
            reference_count, 1,
            "after a reallocation the frames should not be shared",
        );
    }

    let len = cmp::min(frames.len(), prev_frames.len());

    assert_eq!(
        frames[.. len],
        prev_frames[.. len],
        "the paged allocator should reuse existing frames on reallocations",
    );

    frames
}

fn validate_initialization<T: Memory<Address = Virt>>(
    block: Block<T>,
    initialize: Initialize,
) {
    if initialize == Initialize::Zero {
        let slice = unsafe { block.try_into_slice::<MaybeUninit<u8>>().unwrap().assume_init_ref() };
        assert!(slice.iter().all(|&x| x == 0));
    }
}

macro_rules! my_assert {
    ($cond:expr $(,)?) => {{
        assert!($cond);
    }};
}

include!("include/memory_allocator.rs");
