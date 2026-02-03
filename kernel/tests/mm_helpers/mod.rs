#![allow(dead_code)]

use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use ku::{
    error::{
        Error::{
            NoPage,
            Unimplemented,
        },
        Result,
    },
    log::debug,
    memory::{
        FULL_ACCESS,
        Frame,
        KERNEL_R,
        KERNEL_RW,
        Page,
        Phys,
        USER_R,
        USER_RW,
        Virt,
        mmu::{
            PAGE_OFFSET_BITS,
            PAGE_TABLE_ENTRY_COUNT,
            PAGE_TABLE_LEAF_LEVEL,
            PAGE_TABLE_ROOT_LEVEL,
            PageTableEntry,
            PageTableFlags,
        },
        size::{
            self,
            MiB,
        },
    },
};

use kernel::memory::{
    AddressSpace,
    BASE_ADDRESS_SPACE,
    FRAME_ALLOCATOR,
    FrameGuard,
    test_scaffolding::{
        self,
        deepest_pte,
        get_pte,
        nodes,
        page_table_root,
        path,
        phys2virt,
        translate,
    },
};

pub(super) fn check_intermediate_flags(
    address_space: &mut AddressSpace,
    virt: Virt,
    expected_flags: PageTableFlags,
) {
    let path = path(address_space, virt);
    let intermediate_flags =
        unsafe { nodes(&path)[size::from(PAGE_TABLE_ROOT_LEVEL)].unwrap().as_ref().flags() };
    assert_eq!(
        intermediate_flags, expected_flags,
        "intermediate page table flags are incorrect",
    );

    let leaf_flags = PageTableFlags::EXECUTABLE;
    let pte = translate(address_space, virt).unwrap();
    assert_eq!(
        pte.flags(),
        leaf_flags,
        "leaf page table flags are incorrect",
    );
}

pub(super) fn check_map_intermediate(virt: Virt) {
    let start_free_frames = FRAME_ALLOCATOR.lock().count();

    {
        let address_space = &mut BASE_ADDRESS_SPACE.lock();
        let path = path(address_space, virt);
        let start_root_pte_flags =
            unsafe { nodes(&path)[size::from(PAGE_TABLE_ROOT_LEVEL)].unwrap().as_ref().flags() };
        assert!(!start_root_pte_flags.is_present());
    }

    let pte = map_intermediate(virt, PageTableFlags::PRESENT).unwrap();

    debug!(?pte);

    let end_free_frames = FRAME_ALLOCATOR.lock().count();
    assert!(start_free_frames > end_free_frames);

    check_intermediate_flags(
        &mut BASE_ADDRESS_SPACE.lock(),
        virt,
        PageTableFlags::PRESENT,
    );

    for flags in [KERNEL_RW, USER_RW] {
        map_intermediate(virt, flags).unwrap();
        assert_eq!(FRAME_ALLOCATOR.lock().count(), end_free_frames);
        check_intermediate_flags(&mut BASE_ADDRESS_SPACE.lock(), virt, flags);
    }
}

pub(super) fn check_path(
    address_space: &mut AddressSpace,
    virt: Virt,
) {
    let page_table_root = Page::new(Virt::from_ref(page_table_root(address_space))).unwrap();
    let path = path(address_space, virt);

    let page = Page::containing(
        nodes(&path)[size::from(PAGE_TABLE_ROOT_LEVEL)]
            .map(|node| Virt::from_ptr(node.as_ptr()))
            .unwrap_or_default(),
    );
    assert_eq!(
        page, page_table_root,
        "PTE at PAGE_TABLE_ROOT_LEVEL in any Path should be the inside the root page table",
    );

    assert!(
        nodes(&path).iter().is_partitioned(Option::is_none),
        "only lower PTEs can be absent in a Path",
    );

    let (level, pte) = deepest_pte(&path);
    if level > 0 {
        assert!(!pte.is_present() || pte.is_huge());
    }

    for (&child, &parent) in nodes(&path).iter().zip(nodes(&path)[1 ..].iter()) {
        if let Some(child) = child {
            let child_page = Page::containing(Virt::from_ptr(child.as_ptr()));
            let phys2virt_start_page =
                Page::containing(phys2virt(address_space).map(Phys::default()).unwrap());
            let child_frame_index = (child_page - phys2virt_start_page).unwrap();
            let parent_points_to = unsafe { parent.unwrap().as_ref().frame().unwrap() };
            assert_eq!(child_frame_index, parent_points_to.index());
        }
    }
}

#[cfg(feature = "forbid-leaks")]
#[must_use]
pub fn forbid_frame_leaks() -> impl Drop {
    use kernel::memory::Translate;

    BASE_ADDRESS_SPACE.lock().unmap_unused_intermediate();

    scopeguard::guard(FRAME_ALLOCATOR.lock().count(), |start_free_frames| {
        BASE_ADDRESS_SPACE.lock().unmap_unused_intermediate();

        let end_free_frames = FRAME_ALLOCATOR.lock().count();

        let (message, affected_frames) = if start_free_frames <= end_free_frames {
            (
                "freed more memory than allocated",
                end_free_frames - start_free_frames,
            )
        } else {
            ("leaked some memory", start_free_frames - end_free_frames)
        };

        if affected_frames != 0 {
            ku::log::error!(start_free_frames, end_free_frames, affected_frames, message);
            panic!("{}", message);
        }
    })
}

#[cfg(not(feature = "forbid-leaks"))]
#[must_use]
pub fn forbid_frame_leaks() -> impl Drop {
    scopeguard::guard(0, |_| {})
}

pub(super) fn map_intermediate(
    virt: Virt,
    flags: PageTableFlags,
) -> Result<PageTableEntry> {
    let mut address_space = BASE_ADDRESS_SPACE.lock();

    let frame = FrameGuard::allocate().unwrap();
    let mut path = path(&mut address_space, virt);
    let map_result = unsafe { path.map(frame, flags) };
    map_result?;
    let pte = *get_pte(&path)?;
    unsafe {
        path.unmap()?;
    }

    Ok(pte)
}

pub(super) fn test_no_excessive_intermediate_flags(virt: Virt) {
    let start_free_frames = FRAME_ALLOCATOR.lock().count();

    let flags = PageTableFlags::all().difference(FULL_ACCESS | PageTableFlags::HUGE);
    map_intermediate(virt, flags).unwrap();

    let end_free_frames = FRAME_ALLOCATOR.lock().count();
    assert!(start_free_frames > end_free_frames);

    check_intermediate_flags(
        &mut BASE_ADDRESS_SPACE.lock(),
        virt,
        PageTableFlags::PRESENT,
    );
}

pub(super) fn test_path(address_space: &mut AddressSpace) {
    static VARIABLE: AtomicUsize = AtomicUsize::new(314159265);
    let present_virt = Virt::from_ref(&VARIABLE);
    let present_path = path(address_space, present_virt);
    let (level, pte) = deepest_pte(&present_path);
    debug!("present:");
    debug!("    virt = {present_virt}");
    debug!("    path = {present_path}");
    debug!("    level = {level}");
    debug!("    pte = {pte:?}");
    assert_eq!(level, PAGE_TABLE_LEAF_LEVEL);
    assert!(pte.is_present());
    assert!(get_pte(&present_path).is_ok());
    check_path(address_space, present_virt);

    let non_present_virt = unique_user_virt();
    let non_present_path = path(address_space, non_present_virt);
    let (level, pte) = deepest_pte(&non_present_path);
    debug!("non-present:");
    debug!("    virt = {non_present_virt}");
    debug!("    path = {non_present_path}");
    debug!("    level = {level}");
    debug!("    pte = {pte:?}");
    assert!(level > PAGE_TABLE_LEAF_LEVEL);
    assert_ne!(
        Virt::from_ref(pte),
        Page::containing(Virt::from_ref(pte)).address(),
        "nodes should be addresses of PageTableEntries, not addresses of PageTables",
    );
    assert!(!pte.is_present());
    assert_eq!(get_pte(&non_present_path), Err(NoPage));
    check_path(address_space, non_present_virt);

    let huge_virt = phys2virt(address_space).map(Phys::new(7 * MiB).unwrap()).unwrap();
    let huge_path = path(address_space, huge_virt);
    let (level, pte) = deepest_pte(&huge_path);
    debug!("huge:");
    debug!("    virt = {huge_virt}");
    debug!("    path = {huge_path}");
    debug!("    level = {level}");
    debug!("    pte = {pte:?}");
    assert_ne!(
        level, PAGE_TABLE_LEAF_LEVEL,
        "a Path should not descend deeper into a huge page",
    );
    assert!(pte.is_present());
    assert!(pte.is_huge());
    assert_eq!(get_pte(&huge_path), Err(Unimplemented));
    check_path(address_space, huge_virt);
}

pub(super) fn test_translate(address_space: &mut AddressSpace) {
    static VARIABLE: AtomicUsize = AtomicUsize::new(314159265);
    VARIABLE.fetch_add(1, Ordering::Relaxed);
    let write_ptr = &VARIABLE;

    let virt = Virt::from_ptr(&raw const VARIABLE);

    let pte = translate(address_space, virt).unwrap();

    debug!(?pte);

    let frame = pte.frame().unwrap();
    let expected_flags = PageTableFlags::PRESENT |
        PageTableFlags::WRITABLE |
        PageTableFlags::ACCESSED |
        PageTableFlags::DIRTY;

    assert_eq!(pte.flags(), expected_flags);
    assert_ne!(frame, Frame::from_index(0).unwrap());

    let phys2virt = phys2virt(address_space);

    let page_offset_mask = (1 << PAGE_OFFSET_BITS) - 1;
    let page_offset = virt.into_usize() & page_offset_mask;
    let page_virt = phys2virt.map(frame.address()).unwrap();
    let alternative_virt = (page_virt + page_offset).unwrap();
    let read_ptr: *const AtomicUsize = alternative_virt.try_into_ptr().unwrap();

    debug!(?read_ptr, ?write_ptr);
    assert_ne!(read_ptr, write_ptr);

    const ITERATIONS: usize = 5;

    for write_value in 0 .. ITERATIONS {
        let read_value = unsafe {
            write_ptr.store(write_value, Ordering::Relaxed);
            read_ptr.read_volatile().load(Ordering::Relaxed)
        };
        let variable = VARIABLE.load(Ordering::Relaxed);
        debug!(write_value, read_value, variable);
        assert_eq!(read_value, write_value);
        assert_eq!(read_value, variable);
    }
}

pub(super) fn unique_kernel_virt() -> Virt {
    unique_virt(KERNEL_R)
}

pub(super) fn unique_user_virt() -> Virt {
    unique_virt(USER_R)
}

pub(super) fn unique_virt(flags: PageTableFlags) -> Virt {
    const ROOT_LEVEL_ENTRY_SIZE: usize =
        PAGE_TABLE_ENTRY_COUNT.pow(PAGE_TABLE_ROOT_LEVEL) * Page::SIZE;

    static UNIQUE_ROOT_LEVEL_ENTRY: AtomicUsize = AtomicUsize::new(1);

    let is_user = flags.is_user();
    let start = test_scaffolding::user_pages().start_address();
    let offset = UNIQUE_ROOT_LEVEL_ENTRY.fetch_add(1, Ordering::Relaxed) * ROOT_LEVEL_ENTRY_SIZE;
    let virt = (if is_user {
        start + offset
    } else {
        start - offset
    })
    .unwrap();

    debug!(%virt, is_user, "allocated unique virtual address");

    virt
}
