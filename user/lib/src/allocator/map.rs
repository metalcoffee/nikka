use core::{
    alloc::{
        AllocError,
        Allocator,
        Layout,
    },
    ptr::NonNull,
    result,
};

use ku::{
    allocator::{
        BigAllocator,
        BigAllocatorGuard,
        DryAllocator,
        Initialize,
    },
    error::{
        Error::{
            InvalidAlignment,
            InvalidArgument,
            PermissionDenied,
        },
        Result,
    },
    memory::{
        Block,
        Page,
        SizeOf,
        USER_RW,
        mmu::PageTableFlags,
    },
    process::Pid,
};

use crate::syscall;

/// Аллокатор памяти общего назначения в пространстве пользователя, реализованный через
/// системные вызовы [`syscall::map()`], [`syscall::unmap()`] и [`syscall::copy_mapping()`].
#[derive(Clone)]
pub(super) struct MapAllocator {
    /// Флаги доступа к выделяемой аллокатором памяти.
    flags: PageTableFlags,
}

impl MapAllocator {
    /// Аллокатор памяти общего назначения в пространстве пользователя, реализованный через
    /// системные вызовы [`syscall::map()`], [`syscall::unmap()`] и [`syscall::copy_mapping()`].
    pub(super) const fn new() -> Self {
        Self { flags: USER_RW }
    }
}

unsafe impl BigAllocator for MapAllocator {
    fn flags(&self) -> PageTableFlags {
        self.flags
    }

    fn set_flags(
        &mut self,
        flags: PageTableFlags,
    ) -> Result<()> {
        if flags.is_user() {
            self.flags = flags;

            Ok(())
        } else {
            Err(PermissionDenied)
        }
    }

    fn reserve(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>> {
        if layout.align() <= Page::SIZE {
            let block = Block::from_index(0, Page::count_up(layout.size()))?;
            syscall::map(Pid::Current, block, PageTableFlags::USER)
        } else {
            Err(InvalidAlignment)
        }
    }

    fn reserve_fixed(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        syscall::map(Pid::Current, block, PageTableFlags::USER).map(|_| ())
    }

    unsafe fn unreserve(
        &mut self,
        _block: Block<Page>,
    ) -> Result<()> {
        Ok(())
    }

    unsafe fn rereserve(
        &mut self,
        old_block: Block<Page>,
        sub_block: Block<Page>,
    ) -> Result<()> {
        if old_block.contains_block(sub_block) {
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
        syscall::map(Pid::Current, block, flags).map(|_| ())
    }

    unsafe fn unmap(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        if block.count() > 0 {
            syscall::unmap(Pid::Current, block)
        } else {
            Ok(())
        }
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        syscall::copy_mapping(Pid::Current, old_block, new_block, flags)
    }
}

impl BigAllocatorGuard for MapAllocator {
    fn get(&self) -> impl BigAllocator {
        self.clone()
    }
}

unsafe impl Allocator for MapAllocator {
    fn allocate(
        &self,
        layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        Self::new().dry_allocate(layout, Initialize::Garbage).map_err(|_| AllocError)
    }

    fn allocate_zeroed(
        &self,
        layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        Self::new().dry_allocate(layout, Initialize::Zero).map_err(|_| AllocError)
    }

    unsafe fn deallocate(
        &self,
        ptr: NonNull<u8>,
        layout: Layout,
    ) {
        unsafe {
            Self::new().dry_deallocate(ptr, layout);
        }
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        unsafe {
            Self::new()
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
            Self::new()
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
        unsafe { Self::new().dry_shrink(ptr, old_layout, new_layout).map_err(|_| AllocError) }
    }
}
