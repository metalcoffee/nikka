#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(iter_is_partitioned)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::{
    log::debug,
    memory::{
        Block,
        Frame,
        KERNEL_RW,
        L2_SIZE,
        Page,
        Phys,
        Virt,
        mmu::{
            self,
            PAGE_TABLE_ENTRY_COUNT,
            PageTable,
            PageTableEntry,
        },
        size::{
            GiB,
            MiB,
            TiB,
        },
    },
};

use kernel::{
    Subsystems,
    memory::test_scaffolding::{
        PAGES_PER_ROOT_LEVEL_ENTRY,
        make_phys2virt,
    },
};

mod init;

init!(Subsystems::PHYS_MEMORY);

#[test_case]
fn basic() {
    let mut iteration = 0;

    if let Some(recursive_mapping) = init::recursive_mapping() {
        for physical_memory_size in itertools::chain(
            itertools::chain(
                (128 * MiB .. 4 * GiB).step_by(3 * MiB),
                (4 * GiB .. 512 * GiB).step_by(512 * MiB),
            ),
            itertools::chain(
                (512 * GiB .. 40 * TiB).step_by(256 * GiB),
                (40 * TiB .. 64 * TiB).step_by(2 * TiB),
            ),
        ) {
            let frame_count = physical_memory_size / Frame::SIZE;
            let physical_memory = Block::from_index(0, frame_count).unwrap();
            let phys2virt = make_phys2virt(physical_memory, recursive_mapping).unwrap();
            let phys2virt_start = phys2virt.map(Phys::default()).unwrap();
            let root_entries_count = frame_count.div_ceil(PAGES_PER_ROOT_LEVEL_ENTRY);
            let root_entries_start = root_entry_for_virt(phys2virt_start);
            let root_entries_end = root_entries_start + root_entries_count;
            let entry_count = physical_memory.size().div_ceil(GiB);

            let log_frequency = if root_entries_count > 1 {
                20
            } else {
                1000
            };
            if iteration % log_frequency == 0 {
                debug!(iteration, %phys2virt, root_entries_start, root_entries_end, entry_count);
            }
            iteration += 1;

            let root = unsafe {
                phys2virt
                    .map(mmu::page_table_root().address())
                    .unwrap()
                    .try_into_mut::<PageTable>()
                    .unwrap()
            };

            if root_entries_end < PAGE_TABLE_ENTRY_COUNT {
                assert_eq!(root[root_entries_end], PageTableEntry::default());
            }

            let mut entry = 0;
            for root_entry in &root[root_entries_start .. root_entries_end] {
                assert!(root_entry.flags().contains(KERNEL_RW));

                let frame = root_entry.frame().unwrap();
                let virt = phys2virt.map(frame.address()).unwrap();
                let node = unsafe { virt.try_into_ref::<PageTable>().unwrap() };
                for pte in node {
                    if entry < entry_count {
                        assert!(pte.is_huge());
                        assert!(pte.flags().contains(KERNEL_RW));
                        let l2_frame = pte.huge_frame::<L2_SIZE>().unwrap();
                        assert_eq!(l2_frame.address().into_usize(), entry * L2_SIZE);
                    } else {
                        assert_eq!(*pte, PageTableEntry::default());
                    }
                    entry += 1;
                }
            }

            root[root_entries_start .. root_entries_end].fill(PageTableEntry::default());
            unsafe {
                mmu::set_page_table_root(mmu::page_table_root());
            }
        }
    } else {
        debug!("no recursive mapping, skipping the test");
    }

    fn root_entry_for_virt(virt: Virt) -> usize {
        if virt < Virt::higher_half() {
            virt.into_usize() / Page::SIZE / PAGES_PER_ROOT_LEVEL_ENTRY
        } else {
            (virt - Virt::higher_half()).unwrap() / Page::SIZE / PAGES_PER_ROOT_LEVEL_ENTRY +
                PAGE_TABLE_ENTRY_COUNT / 2
        }
    }
}
