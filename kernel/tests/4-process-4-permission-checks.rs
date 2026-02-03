#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    error::Error::NoPage,
    log::debug,
    memory::{
        AddressSpace,
        BASE_ADDRESS_SPACE,
        Block,
        Page,
        USER_R,
        USER_RW,
        Virt,
        mmu::PageTableFlags,
        test_scaffolding::{
            check_permission,
            check_permission_mut,
        },
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::MEMORY);

#[test_case]
fn user_rw() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();
    let page_count = 4;

    let pages = address_space.allocate(Page::layout_array(page_count), USER_RW).unwrap();

    debug!(%pages);
    assert_eq!(pages.count(), page_count);

    unsafe {
        address_space.map_block(pages, USER_RW).unwrap();
    }

    check_permission_mut::<u8>(&mut address_space, pages.into(), USER_RW)
        .expect("test block should be writable for the user space")
        .fill(TEST_VALUE);

    let slice = check_permission::<u8>(&mut address_space, pages.into(), USER_R)
        .expect("test block should be readable for the user space");

    for x in slice {
        assert_eq!(*x, TEST_VALUE);
    }

    unsafe {
        address_space.unmap_block(pages).unwrap();
    }
}

#[test_case]
fn non_present() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();
    let address_filter = |x: &Virt| (x.into_usize() + 2) % (Page::SIZE / 4) <= 4;
    let page_count = 4;

    let pages = address_space.allocate(Page::layout_array(page_count), USER_RW).unwrap();

    debug!(%pages);
    assert_eq!(pages.count(), page_count);

    for start in Block::<Virt>::from(pages).into_iter().filter(address_filter) {
        for end in Block::new(start, (pages.end_address().unwrap() + 1).unwrap())
            .unwrap()
            .into_iter()
            .filter(address_filter)
        {
            let block = Block::new(start, end).unwrap();
            let slice = check_permission::<u8>(&mut address_space, block, USER_R);
            let expected = if block.size() == 0 {
                Ok(unsafe { block.try_into_slice().unwrap() })
            } else {
                Err(NoPage)
            };

            if slice != expected {
                debug!(%block, ?slice, ?expected);
            }

            assert_eq!(slice, expected);
        }
    }
}

#[test_case]
fn stress() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = BASE_ADDRESS_SPACE.lock();
    let address_filter = |x: &Virt| (x.into_usize() + 2) % (Page::SIZE / 4) <= 4;

    for page_count in 1 ..= 4 {
        let pages = address_space.allocate(Page::layout_array(page_count), USER_RW).unwrap();

        debug!(%pages);
        assert_eq!(pages.count(), page_count);

        unsafe {
            address_space.map_block(pages, USER_RW).unwrap();
        }

        for start in Block::<Virt>::from(pages).into_iter().filter(address_filter) {
            for end in Block::new(start, (pages.end_address().unwrap() + 1).unwrap())
                .unwrap()
                .into_iter()
                .filter(address_filter)
            {
                let block = Block::new(start, end).unwrap();
                match check_permission_mut::<u8>(&mut address_space, block, USER_RW) {
                    Ok(slice) => slice.fill(TEST_VALUE),
                    Err(error) => {
                        debug!(%block, ?error);
                        panic!("test block should be writable");
                    },
                }
            }
        }

        for hole_flags in [USER_R, PageTableFlags::empty()] {
            for hole_start in pages {
                for hole_end in pages.into_iter().filter(|hole_end| hole_start < *hole_end) {
                    let hole = Block::from_index(hole_start.index(), hole_end.index() + 1).unwrap();
                    unsafe {
                        if hole_flags.is_present() {
                            address_space.map_block(hole, hole_flags).unwrap();
                        } else {
                            address_space.unmap_block(hole).unwrap();
                        }
                    }

                    for start in Block::<Virt>::from(pages).into_iter().filter(address_filter) {
                        for end in Block::new(start, (pages.end_address().unwrap() + 1).unwrap())
                            .unwrap()
                            .into_iter()
                            .filter(address_filter)
                        {
                            let block = Block::new(start, end).unwrap();
                            validate_permissions(&mut address_space, block, hole, hole_flags);
                        }
                    }

                    unsafe {
                        if hole_flags.is_present() {
                            address_space.unmap_block(hole).unwrap();
                        }
                        address_space.map_block(hole, USER_RW).unwrap();
                    }
                }
            }
        }

        unsafe {
            address_space.unmap_block(pages).unwrap();
        }
    }

    fn validate_permissions(
        address_space: &mut AddressSpace,
        block: Block<Virt>,
        hole: Block<Page>,
        hole_flags: PageTableFlags,
    ) {
        let intersects = block.size() > 0 &&
            block.start_address() < hole.end_address().unwrap() &&
            hole.start_address() < block.end_address().unwrap();
        let readable = hole_flags.contains(USER_R) || !intersects;
        let writable = !intersects;

        let check_readable = check_permission::<u8>(address_space, block, USER_R).is_ok();
        if readable != check_readable {
            debug!(%block, %hole, readable, check_readable);
        }
        assert_eq!(readable, check_readable);

        let check_writable = check_permission_mut::<u8>(address_space, block, USER_RW).is_ok();
        if writable != check_writable {
            debug!(%block, %hole, writable, check_writable);
        }
        assert_eq!(writable, check_writable);
    }
}

static TEST_VALUE: u8 = 77;
