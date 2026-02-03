#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::error::Error::NoPage;

use kernel::{
    Subsystems,
    log::debug,
    memory::{
        AddressSpace,
        BASE_ADDRESS_SPACE,
        FRAME_ALLOCATOR,
        FrameGuard,
        KERNEL_RW,
        Page,
        USER_R,
        USER_RW,
        USER_RX,
        Virt,
        mmu::{
            PAGE_TABLE_ENTRY_COUNT,
            PAGE_TABLE_ROOT_LEVEL,
        },
        test_scaffolding::{
            duplicate,
            map_page,
            path,
            switch_to,
            translate,
            unmap_page,
            user_root_level_entries,
        },
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::PHYS_MEMORY | Subsystems::VIRT_MEMORY);

#[test_case]
fn basic() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let variable =
        unsafe { BASE_ADDRESS_SPACE.lock().map_slice_zeroed::<usize>(1, KERNEL_RW).unwrap() };
    let virt = Virt::from_ref(&variable[0]);
    let frame = translate(&mut BASE_ADDRESS_SPACE.lock(), virt).unwrap().frame().unwrap();

    let reference_count = FRAME_ALLOCATOR.lock().reference_count(frame).unwrap();
    debug!(%virt, %frame, %reference_count, "one address space");
    assert_eq!(reference_count, 1);

    let mut address_space_copy = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();
    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let reference_count = FRAME_ALLOCATOR.lock().reference_count(frame).unwrap();
    debug!(%virt, %frame, %reference_count, "two identical address spaces");
    assert_eq!(reference_count, 2);

    let ptes = [
        translate(&mut address_space, virt).unwrap(),
        translate(&mut address_space_copy, virt).unwrap(),
    ];
    let pte_addresses = [Virt::from_ref(ptes[0]), Virt::from_ref(ptes[1])];

    debug!(?ptes);
    debug!(?pte_addresses);

    assert_eq!(ptes[0], ptes[1]);
    assert_ne!(pte_addresses[0], pte_addresses[1]);

    drop(address_space_copy);

    let reference_count = FRAME_ALLOCATOR.lock().reference_count(frame).unwrap();
    debug!(%virt, %frame, %reference_count, "back to one address space");
    if cfg!(feature = "forbid-leaks") {
        assert_eq!(reference_count, 1);
    }

    unsafe {
        address_space.unmap_slice(variable).unwrap();
    }
}

#[test_case]
fn duplicate_and_drop() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let start_free_frames = FRAME_ALLOCATOR.lock().count();

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let a = mm_helpers::unique_user_virt();
    let b = mm_helpers::unique_user_virt();

    {
        let frame = FrameGuard::allocate().unwrap();
        let mut path = path(&mut address_space, a);
        unsafe {
            path.map(frame, USER_R).unwrap();
            path.unmap().unwrap();
        }
        mm_helpers::check_intermediate_flags(&mut address_space, a, USER_R);
    }

    let free_frames = map_in_duplicate(&mut address_space, false, true, a);
    assert!(start_free_frames > free_frames);

    {
        let pte = translate(&mut address_space, b);
        assert_eq!(pte, Err(NoPage));
    }

    let end_free_frames = map_in_duplicate(&mut address_space, true, false, b);
    assert!(free_frames > end_free_frames);

    fn map_in_duplicate(
        address_space: &mut AddressSpace,
        allocate_intermediate: bool,
        mapped_in_original: bool,
        virt: Virt,
    ) -> usize {
        let mut address_space_copy = duplicate(address_space).unwrap();

        {
            let start_free_frames = FRAME_ALLOCATOR.lock().count();
            let frame = FrameGuard::allocate().unwrap();
            let mut path = path(&mut address_space_copy, virt);
            unsafe {
                path.map(frame, USER_R).unwrap();
                path.unmap().unwrap();
            }
            let end_free_frames = FRAME_ALLOCATOR.lock().count();
            assert!(allocate_intermediate || end_free_frames == start_free_frames);
        }

        mm_helpers::check_intermediate_flags(&mut address_space_copy, virt, USER_R);
        if mapped_in_original {
            mm_helpers::check_intermediate_flags(address_space, virt, USER_R);
        }

        FRAME_ALLOCATOR.lock().count()
    }
}

#[test_case]
fn duplicate_path() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space_copy = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();
    switch_to(&address_space_copy);

    mm_helpers::test_path(&mut address_space_copy);
}

#[test_case]
fn duplicate_translate() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space_copy = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();
    switch_to(&address_space_copy);

    mm_helpers::test_translate(&mut address_space_copy);
}

#[test_case]
fn all_frames_are_freed_on_drop() {
    const PAGES_PER_ROOT_LEVEL_ENTRY: usize = PAGE_TABLE_ENTRY_COUNT.pow(PAGE_TABLE_ROOT_LEVEL);

    let _guard = mm_helpers::forbid_frame_leaks();

    let mut address_space = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();

    for (i, flags) in [USER_R, USER_RW, USER_RX].into_iter().enumerate() {
        let page =
            Page::from_index((user_root_level_entries().start + i) * PAGES_PER_ROOT_LEVEL_ENTRY)
                .unwrap();
        unsafe {
            map_page(&mut address_space, page, flags).unwrap();
        }
        let frame = translate(&mut address_space, page.address()).unwrap().frame().unwrap();
        debug!(%page, %frame, ?flags);
    }
}

#[test_case]
fn garbage_in_duplicate() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let virt = Virt::new(0x0000_0300_0000_0000).unwrap();
    let page = Page::containing(virt);

    let mut address_space = BASE_ADDRESS_SPACE.lock();

    unsafe {
        map_page(&mut address_space, page, KERNEL_RW).unwrap();
    }

    let mut address_space_copy = duplicate(&address_space).unwrap();

    let ptes = [
        translate(&mut address_space, virt).unwrap(),
        translate(&mut address_space_copy, virt).unwrap(),
    ];
    let pte_addresses = [Virt::from_ref(ptes[0]), Virt::from_ref(ptes[1])];

    debug!(?ptes);
    debug!(?pte_addresses);

    assert_eq!(ptes[0], ptes[1]);
    assert_ne!(pte_addresses[0], pte_addresses[1]);

    for neighbour in 1 .. PAGE_TABLE_ENTRY_COUNT {
        let neighbour_virt = (virt + neighbour * Page::SIZE).unwrap();
        let expected_neighbour_pte = translate(&mut address_space, neighbour_virt).unwrap();
        let neighbour_pte = translate(&mut address_space_copy, neighbour_virt).unwrap();
        if *neighbour_pte != *expected_neighbour_pte {
            debug!(?neighbour_pte, ?expected_neighbour_pte);
        }
        assert_eq!(*neighbour_pte, *expected_neighbour_pte);
    }

    drop(address_space_copy);

    unsafe {
        unmap_page(&mut address_space, page).unwrap();
    }
}

#[test_case]
fn duplicate_only_kernel() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let page = BASE_ADDRESS_SPACE
        .lock()
        .allocate(Page::layout_array(1), USER_R)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let virt = page.address();

    debug!(?page);

    unsafe {
        map_page(&mut BASE_ADDRESS_SPACE.lock(), page, USER_R).unwrap();
    }

    let mut address_space_copy = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();
    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let ptes = [
        translate(&mut address_space, virt).unwrap(),
        translate(&mut address_space_copy, virt).unwrap(),
    ];
    debug!(?ptes);
    assert!(ptes[0].is_present());
    assert!(!ptes[1].is_present(), "do not copy mappings to user pages");

    drop(address_space_copy);

    unsafe {
        unmap_page(&mut address_space, page).unwrap();
    }
}

#[test_case]
fn duplicate_works() {
    let _guard = mm_helpers::forbid_frame_leaks();

    let variable =
        unsafe { BASE_ADDRESS_SPACE.lock().map_slice_zeroed::<usize>(1, KERNEL_RW).unwrap() };
    variable[0] = 0;
    debug!(
        variable = variable[0],
        "original address space initializes the variable",
    );

    let address_space_copy = duplicate(&BASE_ADDRESS_SPACE.lock()).unwrap();
    let mut address_space = BASE_ADDRESS_SPACE.lock();

    switch_to(&address_space_copy);

    debug!("inside the duplicate address space");
    variable[0] = 1;
    debug!(
        variable = variable[0],
        "duplicate address space modifies the variable",
    );

    switch_to(&address_space);

    drop(address_space_copy);

    debug!(
        variable = variable[0],
        "original address space reads the variable",
    );
    assert_eq!(variable[0], 1);

    unsafe {
        address_space.unmap_slice(variable).unwrap();
    }
}
