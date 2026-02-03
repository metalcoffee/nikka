use std::{
    alloc::Layout,
    cmp,
    fs::File,
    num::NonZeroUsize,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

use nix::sys::mman::{
    self,
    MapFlags,
    ProtFlags,
};

use ku::{
    allocator::BigAllocator,
    error::Result,
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
        size::MiB,
    },
};

pub struct CachingBig {
    cache: Block<Page>,
}

impl CachingBig {
    #[allow(unused)]
    pub const fn new() -> Self {
        Self {
            cache: Block::zero(),
        }
    }

    #[allow(unused)]
    pub fn total_memory() -> usize {
        TOTAL_MEMORY.load(Ordering::Relaxed)
    }

    fn reset_total_memory() {
        TOTAL_MEMORY.store(0, Ordering::Relaxed);
    }

    const RESERVE_FACTOR: usize = 16;
    const RESERVE_MIN: usize = 16 * MiB;
}

unsafe impl BigAllocator for CachingBig {
    fn flags(&self) -> PageTableFlags {
        PageTableFlags::default()
    }

    fn set_flags(
        &mut self,
        _flags: PageTableFlags,
    ) -> Result<()> {
        unimplemented!();
    }

    fn reserve(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>> {
        if self.cache == Block::default() {
            Self::reset_total_memory();
        }

        if self.cache.size() < layout.size() {
            let reserve_size = cmp::max(Self::RESERVE_FACTOR * layout.size(), Self::RESERVE_MIN);

            if self.cache.size() > 0 {
                unsafe {
                    mman::munmap(
                        self.cache.start_address().try_into_mut_ptr().unwrap(),
                        self.cache.size(),
                    )
                    .unwrap();
                }
            }

            let ptr = unsafe {
                mman::mmap::<File>(
                    None,
                    NonZeroUsize::new(reserve_size).unwrap(),
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_ANONYMOUS | MapFlags::MAP_PRIVATE,
                    None,
                    0,
                )
                .unwrap()
            };

            self.cache = super::block(ptr, reserve_size);
        }

        Ok(self.cache.tail(layout.size().div_ceil(Page::SIZE)).unwrap())
    }

    fn reserve_fixed(
        &mut self,
        _block: Block<Page>,
    ) -> Result<()> {
        unimplemented!();
    }

    unsafe fn unreserve(
        &mut self,
        _block: Block<Page>,
    ) -> Result<()> {
        Ok(())
    }

    unsafe fn rereserve(
        &mut self,
        _old_block: Block<Page>,
        _sub_block: Block<Page>,
    ) -> Result<()> {
        unimplemented!();
    }

    unsafe fn map(
        &mut self,
        block: Block<Page>,
        _flags: PageTableFlags,
    ) -> Result<()> {
        TOTAL_MEMORY.fetch_add(block.size(), Ordering::Relaxed);

        Ok(())
    }

    unsafe fn unmap(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        TOTAL_MEMORY.fetch_sub(block.size(), Ordering::Relaxed);

        Ok(())
    }

    unsafe fn copy_mapping(
        &mut self,
        _old_block: Block<Page>,
        _new_block: Block<Page>,
        _flags: Option<PageTableFlags>,
    ) -> Result<()> {
        unimplemented!();
    }
}

static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);
