use std::alloc::Layout;

use nix::sys::mman;

use ku::{
    allocator::BigAllocator,
    error::Result,
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
    },
};

use super::Big;

pub struct BigForPipe {
    big: Big,
    reserved: Block<Page>,
    buffer: Block<Page>,
    mirrored_buffer: Block<Page>,
}

#[allow(unused)]
impl BigForPipe {
    pub fn new(check_leaks: bool) -> Self {
        Self {
            big: Big::new(check_leaks),
            reserved: Block::default(),
            buffer: Block::default(),
            mirrored_buffer: Block::default(),
        }
    }

    pub fn unmap(self) {
        // Do not unmap the memory in the `Drop` impl.
        // A drop will happen on any panic.
        // If it unmaps the memory it can cause a SIGSEGV that hides the real problem.
        for block in [self.buffer, self.mirrored_buffer] {
            unsafe {
                mman::munmap(
                    block.start_address().try_into_mut_ptr().unwrap(),
                    block.size(),
                )
                .unwrap();
            }
        }
    }

    pub const GARBAGE: u8 = Big::GARBAGE;
}

unsafe impl BigAllocator for BigForPipe {
    fn flags(&self) -> PageTableFlags {
        self.big.flags()
    }

    fn set_flags(
        &mut self,
        flags: PageTableFlags,
    ) -> Result<()> {
        self.big.set_flags(flags)
    }

    fn reserve(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>> {
        assert_eq!(self.reserved, Block::default());
        assert_eq!(layout.size() % (2 * Page::SIZE), 0);
        assert_eq!(layout.align() % Page::SIZE, 0);

        self.reserved = self.big.reserve(layout)?;

        Ok(self.reserved)
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
        unimplemented!();
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
        flags: PageTableFlags,
    ) -> Result<()> {
        assert_eq!(self.buffer, Block::default());
        assert_eq!(block.size(), self.reserved.size() / 2);
        assert!(self.reserved.contains_block(block));

        unsafe {
            self.big.map(block, flags)?;
        }

        self.buffer = self.big.last();

        Ok(())
    }

    unsafe fn unmap(
        &mut self,
        _block: Block<Page>,
    ) -> Result<()> {
        unimplemented!();
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        assert_ne!(self.buffer, Block::default());
        assert_eq!(self.mirrored_buffer, Block::default());
        assert_eq!(self.buffer, old_block);
        assert!(self.reserved.contains_block(new_block));

        unsafe {
            self.big.copy_mapping(old_block, new_block, flags)?;
        }

        self.mirrored_buffer = self.big.last();

        Ok(())
    }
}
