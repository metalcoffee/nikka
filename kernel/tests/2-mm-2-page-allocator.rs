#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::alloc::Layout;

use ku::memory::{
    mmu::{
        PAGE_TABLE_ENTRY_COUNT,
        PageTableEntry,
        PageTableFlags,
    },
    size::{
        Size,
        TiB,
    },
};

use kernel::{
    Subsystems,
    log::debug,
    memory::{
        BASE_ADDRESS_SPACE,
        KERNEL_R,
        Page,
        USER_R,
        Virt,
        test_scaffolding::{
            LOWER_HALF_ROOT_LEVEL_ENTRY_COUNT,
            find_unused_block,
            page_allocator_block,
        },
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::PHYS_MEMORY | Subsystems::VIRT_MEMORY);

#[test_case]
fn sanity_check() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let page_allocator_block = page_allocator_block(&BASE_ADDRESS_SPACE.lock());

    debug!(?page_allocator_block);

    assert!(page_allocator_block.size() > 100 * TiB);
    assert!(page_allocator_block.size() < 128 * TiB);
}

#[test_case]
fn some_used_entries() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let tests: [&[usize]; 6] = [&[], &[0], &[2], &[31], &[31, 254], &[0, 2, 31, 254]];

    for used_entries in tests.iter() {
        let mut page_table_root = [PageTableEntry::default(); PAGE_TABLE_ENTRY_COUNT];
        for used_entry in used_entries.iter() {
            for half in 0 ..= 1 {
                page_table_root[used_entry + half * PAGE_TABLE_ENTRY_COUNT / 2]
                    .set_flags(PageTableFlags::PRESENT);
            }
        }

        let unused_block =
            find_unused_block(&page_table_root, 0 .. LOWER_HALF_ROOT_LEVEL_ENTRY_COUNT);

        debug!(?used_entries, ?unused_block);

        let unused_block = unused_block.unwrap();

        assert!(unused_block.size() > 100 * TiB);

        if used_entries.is_empty() {
            assert_eq!(unused_block.size(), 128 * TiB);
        } else {
            assert!(unused_block.size() < 128 * TiB);
        }
    }
}

#[test_case]
fn allocate_block() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    for size in (0 ..= 3 * Page::SIZE).filter(|x| (x + 3) % Page::SIZE < 6) {
        let free_page_count = page_allocator_block(&address_space).count();
        let allocated = address_space.allocate(Page::layout(size).unwrap(), USER_R).unwrap();
        let allocated_page_count = free_page_count - page_allocator_block(&address_space).count();

        debug!(requested = %Size::new::<Virt>(size), %allocated, allocated_page_count);

        assert!(
            allocated.size() >= size,
            "allocated less than the requested size",
        );
        assert!(
            allocated.size() - size < Page::SIZE,
            "allocated excessive memory",
        );
        assert_eq!(allocated.count(), allocated_page_count, "lost some pages");
    }
}

#[test_case]
fn allocate_page() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let page = BASE_ADDRESS_SPACE
        .lock()
        .allocate(Page::layout_array(1), KERNEL_R)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    debug!(?page);

    assert_ne!(page.index(), 0);
}

#[test_case]
fn allocate_two_pages() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let layout = Page::layout_array(1);
    let pages = [
        address_space.allocate(layout, KERNEL_R).unwrap().into_iter().next().unwrap(),
        address_space.allocate(layout, KERNEL_R).unwrap().into_iter().next().unwrap(),
    ];

    debug!(?pages);

    assert_ne!(pages[0], pages[1]);
}

#[test_case]
fn alignment() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let min_lb_align = 10;
    let max_lb_align = 20;

    for lb_align in min_lb_align ..= max_lb_align {
        let align = 1 << lb_align;
        debug!(align);

        for lb_size in 6 ..= (max_lb_align + 2) {
            let min_size = (1_usize << lb_size).saturating_sub(10) + 1;
            let max_size = (1 << lb_size) + 10;

            for size in min_size ..= max_size {
                let layout = Layout::from_size_align(size, align).unwrap();
                let pages = address_space.allocate(layout, KERNEL_R).unwrap();

                assert_eq!(pages.count(), size.div_ceil(Page::SIZE));
                assert!(pages.start_address().into_usize().is_multiple_of(align));
            }
        }
    }
}
