use std::{
    alloc::{
        AllocError,
        Allocator,
        Layout,
    },
    ffi::c_void,
    ptr::NonNull,
    result,
};

use duplicate::duplicate_item;

use ku::{
    allocator::{
        BigAllocator,
        BigAllocatorGuard,
    },
    error::Result,
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
    },
    sync::{
        Spinlock,
        SpinlockGuard,
    },
};

#[cfg(not(feature = "benchmark"))]
use super::Big;

#[cfg(feature = "benchmark")]
use super::CachingBig as Big;

pub struct Fallback {
    big: Spinlock<Big>,
}

impl Fallback {
    #[allow(unused)]
    pub const fn new() -> Self {
        Self {
            #[cfg(not(feature = "benchmark"))]
            big: Spinlock::new(Big::new(false)),
            #[cfg(feature = "benchmark")]
            big: Spinlock::new(Big::new()),
        }
    }
}

unsafe impl Allocator for Fallback {
    fn allocate(
        &self,
        layout: Layout,
    ) -> result::Result<NonNull<[u8]>, AllocError> {
        let ptr = if layout.size() == 0 {
            layout.align() as *mut u8
        } else {
            let mut big = self.big.lock();
            let block = big.reserve(layout).map_err(|_| AllocError)?;
            let flags = big.flags();

            unsafe {
                big.map(block, flags).map_err(|_| AllocError)?;
            }

            block.start_address().try_into_mut_ptr().unwrap()
        };

        NonNull::new(ptr)
            .map(|ptr| NonNull::slice_from_raw_parts(ptr, layout.size()))
            .ok_or(AllocError)
    }

    unsafe fn deallocate(
        &self,
        mut ptr: NonNull<u8>,
        layout: Layout,
    ) {
        unsafe {
            let block = super::block(ptr.as_mut() as *mut u8 as *mut c_void, layout.size());
            self.big.lock().unmap(block).unwrap();
        }
    }
}

impl BigAllocatorGuard for Fallback {
    fn get(&self) -> impl BigAllocator {
        BigGuard {
            big: self.big.lock(),
        }
    }
}

struct BigGuard<'a> {
    big: SpinlockGuard<'a, Big>,
}

unsafe impl BigAllocator for BigGuard<'_> {
    fn flags(&self) -> PageTableFlags {
        self.big.flags()
    }

    #[duplicate_item(
        method argument_type return_type;
        [set_flags] [PageTableFlags] [()];
        [reserve] [Layout] [Block<Page>];
        [reserve_fixed] [Block<Page>] [()];
    )]
    fn method(
        &mut self,
        argument: argument_type,
    ) -> Result<return_type> {
        self.big.method(argument)
    }

    #[duplicate_item(
        method;
        [unreserve];
        [unmap];
    )]
    unsafe fn method(
        &mut self,
        argument: Block<Page>,
    ) -> Result<()> {
        unsafe { self.big.method(argument) }
    }

    #[duplicate_item(
        method second_argument_type;
        [rereserve] [Block<Page>];
        [map] [PageTableFlags];
    )]
    unsafe fn method(
        &mut self,
        first_argument: Block<Page>,
        second_argument: second_argument_type,
    ) -> Result<()> {
        unsafe { self.big.method(first_argument, second_argument) }
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        unsafe { self.big.copy_mapping(old_block, new_block, flags) }
    }
}
