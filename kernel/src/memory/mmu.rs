use x86::tlb;

pub use ku::memory::mmu::{
    FULL_ACCESS,
    KERNEL_MMIO,
    KERNEL_R,
    KERNEL_RW,
    KERNEL_RX,
    PAGE_OFFSET_BITS,
    PAGE_TABLE_ENTRY_COUNT,
    PAGE_TABLE_INDEX_BITS,
    PAGE_TABLE_INDEX_MASK,
    PAGE_TABLE_LEAF_LEVEL,
    PAGE_TABLE_LEVEL_COUNT,
    PAGE_TABLE_ROOT_LEVEL,
    PageTable,
    PageTableEntry,
    PageTableFlags,
    SYSCALL_ALLOWED_FLAGS,
    USER_R,
    USER_RW,
    USER_RX,
    page_table_root,
    set_page_table_root,
};

use ku::memory::Page;

/// Сбрасывает кэш страничного преобразования процессора
/// ([Translation Lookaside Buffer, TLB](https://en.wikipedia.org/wiki/Translation_lookaside_buffer))
/// для заданной страницы `page`.
///
/// # Safety
///
/// Вызывающий код должен работать в режиме ядра
/// (в [кольце защиты](https://en.wikipedia.org/wiki/Protection_ring) 0).
pub unsafe fn flush(page: Page) {
    unsafe {
        tlb::flush(page.address().into_usize());
    }
}
