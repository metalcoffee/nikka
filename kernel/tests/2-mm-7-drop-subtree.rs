#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use kernel::{
    Subsystems,
    memory::{
        BASE_ADDRESS_SPACE,
        FRAME_ALLOCATOR,
        test_scaffolding::unmap_unused_intermediate,
    },
};

mod init;
mod mm_helpers;

init!(Subsystems::PHYS_MEMORY | Subsystems::VIRT_MEMORY);

#[test_case]
fn drop_subtree() {
    if !cfg!(feature = "forbid-leaks") {
        return;
    }

    unmap_unused_intermediate(&mut BASE_ADDRESS_SPACE.lock());

    {
        let start_free_frames = FRAME_ALLOCATOR.lock().count();

        mm_helpers::check_map_intermediate(mm_helpers::unique_user_virt());
        assert!(FRAME_ALLOCATOR.lock().count() < start_free_frames);

        let mut address_space = BASE_ADDRESS_SPACE.lock();

        unmap_unused_intermediate(&mut address_space);
        let mid_free_frames = FRAME_ALLOCATOR.lock().count();

        unmap_unused_intermediate(&mut address_space);
        let end_free_frames = FRAME_ALLOCATOR.lock().count();
        if start_free_frames < end_free_frames {
            panic!(
                "some frames are double freed, check that drop_subtree() clears relevant present \
                 flags",
            );
        }
        assert_eq!(start_free_frames, mid_free_frames);
        assert_eq!(start_free_frames, end_free_frames);
    }

    {
        let _guard = mm_helpers::forbid_frame_leaks();
        mm_helpers::check_map_intermediate(mm_helpers::unique_user_virt());
    }

    {
        let _guard = mm_helpers::forbid_frame_leaks();
        mm_helpers::test_no_excessive_intermediate_flags(mm_helpers::unique_user_virt());
    }
}
