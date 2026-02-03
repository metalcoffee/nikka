use core::alloc::Layout;

use ku::{
    allocator::{
        BigAllocator,
        BigAllocatorPair,
        Info,
    },
    error::{
        Error::InvalidArgument,
        Result,
    },
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
    },
};

use crate::{
    allocator,
    memory::{
        AddressSpace,
        FrameGuard,
        Translate,
    },
};

use super::BigPair;

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// Аллокатор памяти, предназначенный для выделения памяти блоками виртуальных страниц.
pub struct Big<'a> {
    /// Адресное пространство, внутри которого аллокатор выделяет память.
    address_space: &'a mut AddressSpace,

    /// Флаги доступа к выделяемой аллокатором памяти.
    flags: PageTableFlags,
}

impl Big<'_> {
    /// Возвращает аллокатор памяти для постраничного выделения памяти внутри `address_space`.
    /// Выделяемая им память будет отображена с флагами `flags`.
    pub fn new(
        address_space: &mut AddressSpace,
        flags: PageTableFlags,
    ) -> Big<'_> {
        Big {
            address_space,
            flags,
        }
    }
}

unsafe impl BigAllocator for Big<'_> {
    fn flags(&self) -> PageTableFlags {
        self.flags
    }

    fn set_flags(
        &mut self,
        flags: PageTableFlags,
    ) -> Result<()> {
        self.flags = flags;
        Ok(())
    }

    fn reserve(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>> {
        self.address_space.allocate(layout, self.flags)
    }

    fn reserve_fixed(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        self.address_space.reserve(block, self.flags)
    }

    unsafe fn unreserve(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        self.address_space.deallocate(block)
    }

    unsafe fn rereserve(
        &mut self,
        old_block: Block<Page>,
        sub_block: Block<Page>,
    ) -> Result<()> {
        if old_block.contains_block(sub_block) {
            let message = "subblock of a valid block should be valid";
            let left = Block::from_index(old_block.start(), sub_block.start()).expect(message);
            let right = Block::from_index(sub_block.end(), old_block.end()).expect(message);

            if !left.is_empty() {
                unsafe {
                    self.unreserve(left)?;
                }
            }

            if !right.is_empty() {
                unsafe {
                    self.unreserve(right)?;
                }
            }

            Ok(())
        } else {
            Err(InvalidArgument)
        }
    }

    unsafe fn map(
        &mut self,
        block: Block<Page>,
        flags: PageTableFlags,
    ) -> Result<()> {
        unsafe { self.address_space.map_block(block, flags) }
    }

    unsafe fn unmap(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        unsafe { self.address_space.unmap_block(block) }
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        let allocator_flags = self.flags();
        let mut allocator =
            BigPair::new_single(self.address_space, allocator_flags, allocator_flags);

        unsafe { allocator.copy_mapping(old_block, new_block, flags) }
    }
}
