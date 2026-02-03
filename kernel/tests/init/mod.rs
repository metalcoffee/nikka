use core::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use bootloader::bootinfo::MemoryMap;

use ku::memory::mmu::PAGE_TABLE_ENTRY_COUNT;

use kernel::memory::test_scaffolding;

#[macro_export]
macro_rules! init {
    ($subsystems:expr) => {
        bootloader::entry_point!(test_entry);

        #[cfg_attr(
            not(feature = "conservative-backtraces"),
            sentinel_frame::with_sentinel_frame
        )]
        fn test_entry(boot_info: &'static bootloader::BootInfo) -> ! {
            kernel::init_subsystems(&boot_info, $subsystems);
            init::set_frame_count(&boot_info.memory_map);
            init::set_recursive_mapping(ku::memory::size::from(boot_info.recursive_index()));
            test_main();
            panic!("should not return to test_entry()")
        }

        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            ku::sync::start_panicking();
            kernel::fail_test(info)
        }
    };
}

#[allow(dead_code)]
pub(super) fn frame_count() -> usize {
    FRAME_COUNT.load(Ordering::Relaxed)
}

pub(super) fn set_frame_count(memory_map: &MemoryMap) {
    FRAME_COUNT.store(
        test_scaffolding::physical(memory_map).end(),
        Ordering::Relaxed,
    );
}

#[allow(unused)]
pub(super) fn recursive_mapping() -> Option<usize> {
    let recursive_mapping = RECURSIVE_MAPPING.load(Ordering::Relaxed);
    if recursive_mapping < PAGE_TABLE_ENTRY_COUNT {
        Some(recursive_mapping)
    } else {
        None
    }
}

pub(super) fn set_recursive_mapping(recursive_mapping: usize) {
    RECURSIVE_MAPPING.store(recursive_mapping, Ordering::Relaxed);
}

static FRAME_COUNT: AtomicUsize = AtomicUsize::new(0);
static RECURSIVE_MAPPING: AtomicUsize = AtomicUsize::new(usize::MAX);
