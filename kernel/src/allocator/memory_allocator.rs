use core::{
    alloc::{
        AllocError,
        Allocator,
        Layout,
    },
    ptr::NonNull,
    result,
};

use duplicate::duplicate_item;

use ku::{
    allocator::{
        BigAllocator,
        BigAllocatorGuard,
        DryAllocator,
        Initialize,
    },
    error::Result,
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
    },
    sync::spinlock::{
        Spinlock,
        SpinlockGuard,
    },
};

use crate::{
    allocator::Big,
    memory::AddressSpace,
};

/// Аллокатор памяти общего назначения внутри [`AddressSpace`].
pub(crate) struct MemoryAllocator<'a> {
    /// Адресное пространство, внутри которого аллокатор выделяет память.
    address_space: &'a Spinlock<AddressSpace>,

    /// Флаги с которыми будет отображена выделяемая память.
    flags: PageTableFlags,
}

impl MemoryAllocator<'_> {
    /// Возвращает аллокатор памяти общего назначения внутри `address_space`.
    /// Выделяемая им память будет отображена с флагами `flags`.
    pub const fn new(
        address_space: &Spinlock<AddressSpace>,
        flags: PageTableFlags,
    ) -> MemoryAllocator<'_> {
        MemoryAllocator {
            address_space,
            flags,
        }
    }
}

unsafe impl Allocator for MemoryAllocator<'_> {
    fn allocate(
        &self,
        layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        self.address_space
            .lock()
            .allocator(self.flags)
            .dry_allocate(layout, Initialize::Garbage)
            .map_err(|_| AllocError)
    }

    fn allocate_zeroed(
        &self,
        layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        self.address_space
            .lock()
            .allocator(self.flags)
            .dry_allocate(layout, Initialize::Zero)
            .map_err(|_| AllocError)
    }

    unsafe fn deallocate(
        &self,
        ptr: NonNull<u8>,
        layout: Layout,
    ) {
        unsafe {
            self.address_space.lock().allocator(self.flags).dry_deallocate(ptr, layout);
        }
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        unsafe {
            self.address_space
                .lock()
                .allocator(self.flags)
                .dry_grow(ptr, old_layout, new_layout, Initialize::Garbage)
                .map_err(|_| AllocError)
        }
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        unsafe {
            self.address_space
                .lock()
                .allocator(self.flags)
                .dry_grow(ptr, old_layout, new_layout, Initialize::Zero)
                .map_err(|_| AllocError)
        }
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        unsafe {
            self.address_space
                .lock()
                .allocator(self.flags)
                .dry_shrink(ptr, old_layout, new_layout)
                .map_err(|_| AllocError)
        }
    }
}

impl<'a> BigAllocatorGuard for MemoryAllocator<'a> {
    fn get(&self) -> impl BigAllocator {
        MemoryAllocatorGuard {
            address_space: self.address_space.lock(),
            flags: self.flags,
        }
    }
}

/// Аллокатор памяти общего назначения внутри [`AddressSpace`].
pub(crate) struct MemoryAllocatorGuard<'a> {
    /// Адресное пространство, внутри которого аллокатор выделяет память.
    address_space: SpinlockGuard<'a, AddressSpace>,

    /// Флаги с которыми будет отображена выделяемая память.
    flags: PageTableFlags,
}

impl MemoryAllocatorGuard<'_> {
    /// Возвращает обёрнутый в [`MemoryAllocatorGuard`] постраничный аллокатор памяти.
    fn allocator(&mut self) -> Big<'_> {
        self.address_space.allocator(self.flags)
    }
}

unsafe impl BigAllocator for MemoryAllocatorGuard<'_> {
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

    #[duplicate_item(
        method argument_type return_type;
        [reserve] [Layout] [Block<Page>];
        [reserve_fixed] [Block<Page>] [()];
    )]
    fn method(
        &mut self,
        argument: argument_type,
    ) -> Result<return_type> {
        self.allocator().method(argument)
    }

    #[duplicate_item(
        method;
        [unreserve];
        [unmap];
    )]
    unsafe fn method(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        unsafe { self.allocator().method(block) }
    }

    #[duplicate_item(
        method argument_type;
        [rereserve] [Block<Page>];
        [map] [PageTableFlags];
    )]
    unsafe fn method(
        &mut self,
        block: Block<Page>,
        argument: argument_type,
    ) -> Result<()> {
        unsafe { self.allocator().method(block, argument) }
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        unsafe {
            self.address_space
                .allocator(self.flags)
                .copy_mapping(old_block, new_block, flags)
        }
    }
}
