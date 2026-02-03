use std::{
    alloc::Layout,
    ffi::c_void,
    fs::File,
    num::NonZeroUsize,
    sync::atomic::{
        AtomicUsize,
        Ordering,
    },
};

use nix::sys::mman::{
    self,
    MRemapFlags,
    MapFlags,
    ProtFlags,
};

use ku::{
    allocator::BigAllocator,
    error::{
        Error::PermissionDenied,
        Result,
    },
    memory::{
        Block,
        Page,
        USER_RW,
        mmu::PageTableFlags,
    },
};

pub struct Big {
    check_leaks: bool,
    flags: PageTableFlags,
    last: Block<Page>,
    mapped: usize,
    reserved: usize,
}

impl Big {
    pub const fn new(check_leaks: bool) -> Self {
        Self {
            check_leaks,
            flags: USER_RW,
            last: Block::zero(),
            mapped: 0,
            reserved: 0,
        }
    }

    #[allow(unused)]
    pub fn total_mapped() -> usize {
        MAPPED.load(Ordering::Relaxed)
    }

    #[allow(unused)]
    pub fn total_reserved() -> usize {
        RESERVED.load(Ordering::Relaxed)
    }

    pub(super) fn last(&self) -> Block<Page> {
        self.last
    }

    fn account_mapped(
        &mut self,
        block: Block<Page>,
        ptr: *mut c_void,
    ) {
        self.last = super::block(ptr, block.size());
        assert_eq!(self.last, block);

        MAPPED.fetch_add(self.last.count(), Ordering::Relaxed);

        self.mapped = self
            .mapped
            .checked_add(self.last.count())
            .expect("mapped too many physical frames, can not check the balance");
    }

    unsafe fn mmap(
        block: Block<Page>,
        prot_flags: ProtFlags,
    ) -> *mut c_void {
        let address = NonZeroUsize::new(block.start_address().into_usize());
        let size = NonZeroUsize::new(block.size()).expect("useless call to map an empty block");

        let mut flags = MapFlags::MAP_ANONYMOUS;
        flags |= if prot_flags == ProtFlags::PROT_NONE {
            MapFlags::MAP_PRIVATE
        } else {
            MapFlags::MAP_SHARED
        };
        if address.is_some() {
            flags |= MapFlags::MAP_FIXED;
        }

        unsafe { mman::mmap::<File>(address, size, prot_flags, flags, None, 0).unwrap() }
    }

    unsafe fn mmap_accounted(
        &mut self,
        block: Block<Page>,
        prot_flags: ProtFlags,
    ) -> Block<Page> {
        let ptr = unsafe { Self::mmap(block, prot_flags) };
        let result_block = super::block(ptr, block.size());

        if prot_flags == ProtFlags::PROT_NONE {
            RESERVED.fetch_add(result_block.count(), Ordering::Relaxed);

            self.reserved = self
                .reserved
                .checked_add(result_block.count())
                .expect("reserved too many virtual pages, can not check the balance");
        } else {
            self.account_mapped(block, ptr);
        }

        result_block
    }

    pub(super) const GARBAGE: u8 = 0xFF;
}

unsafe impl BigAllocator for Big {
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
        let block =
            Block::from_index(0, layout.pad_to_align().size().div_ceil(Page::SIZE)).unwrap();

        Ok(unsafe { self.mmap_accounted(block, ProtFlags::PROT_NONE) })
    }

    fn reserve_fixed(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        assert!(!block.is_empty());

        let reserved_block = unsafe { self.mmap_accounted(block, ProtFlags::PROT_NONE) };

        assert_eq!(block, reserved_block);

        Ok(())
    }

    unsafe fn unreserve(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        assert!(!block.is_empty());

        RESERVED.fetch_sub(block.count(), Ordering::Relaxed);

        self.reserved = self
            .reserved
            .checked_sub(block.count())
            .expect("unreserved more virtual pages than were reserved");

        unsafe {
            mman::munmap(
                block.start_address().try_into_mut_ptr().unwrap(),
                block.size(),
            )
            .unwrap();
        }

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
        flags: PageTableFlags,
    ) -> Result<()> {
        assert!(!block.is_empty());

        let mut prot_flags = ProtFlags::PROT_READ;
        if flags.is_writable() {
            prot_flags |= ProtFlags::PROT_WRITE;
        }

        unsafe {
            self.last = self.mmap_accounted(block, prot_flags);

            if !cfg!(feature = "benchmark") {
                self.last.try_into_mut_slice().unwrap().fill(Self::GARBAGE);
            }
        }

        Ok(())
    }

    unsafe fn unmap(
        &mut self,
        block: Block<Page>,
    ) -> Result<()> {
        assert!(!block.is_empty());

        MAPPED.fetch_sub(block.count(), Ordering::Relaxed);

        self.mapped = self
            .mapped
            .checked_sub(block.count())
            .expect("unmapped more physical frames than were mapped");

        let ptr = unsafe { Self::mmap(block, ProtFlags::PROT_NONE) };
        let reserved_block = super::block(ptr, block.size());

        assert_eq!(block, reserved_block);

        Ok(())
    }

    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()> {
        assert!(!old_block.is_empty());
        assert_eq!(old_block.size(), new_block.size());
        assert_eq!(flags, None);

        let ptr = unsafe {
            mman::mremap(
                old_block.start_address().try_into_mut_ptr().unwrap(),
                0,
                new_block.size(),
                MRemapFlags::MREMAP_MAYMOVE | MRemapFlags::MREMAP_FIXED,
                Some(new_block.start_address().try_into_mut_ptr().unwrap()),
            )
            .unwrap()
        };

        self.account_mapped(new_block, ptr);

        Ok(())
    }
}

impl Drop for Big {
    fn drop(&mut self) {
        if self.check_leaks {
            assert_eq!(self.mapped, 0, "leaked some physical frames");
            assert_eq!(self.reserved, 0, "leaked some virtual pages");
        }
    }
}

static MAPPED: AtomicUsize = AtomicUsize::new(0);
static RESERVED: AtomicUsize = AtomicUsize::new(0);
