use core::{
    alloc::Layout,
    mem::MaybeUninit,
    ptr::NonNull,
};

use crate::{
    error::Result,
    memory::{
        Block,
        Page,
        Virt,
        mmu::PageTableFlags,
        size::SizeOf,
    },
};

use super::{
    DryAllocator,
    Initialize,
};

// Used in docs.
#[allow(unused)]
use {
    crate::error::Error,
    core::alloc::Allocator,
};

/// Интерфейс аллокатора памяти общего назначения,
/// который умеет выдавать только выровненные на границу страниц блоки памяти.
/// Работает либо в текущем адресном пространстве,
/// либо в другом адресном пространстве.
///
/// # Safety
///
/// Код, который реализует этот типаж должен гарантировать,
/// что инварианты управления памятью в Rust'е не будут нарушены.
/// В частности, [`BigAllocator::reserve()`] не должен выдавать
/// занятые в данный момент блоки виртуальных адресов.
pub unsafe trait BigAllocator {
    /// Возвращает текущие флаги отображения страниц.
    ///
    /// Они используются при выделении памяти из [`BigAllocator`] через интерфейсы
    /// [`core::alloc::Allocator`] или [`DryAllocator`],
    /// которые не принимают на вход флагов отображений страниц.
    fn flags(&self) -> PageTableFlags;

    /// Устанавливает текущие флаги отображения страниц.
    ///
    /// Они используются при выделении памяти из [`BigAllocator`] через интерфейсы
    /// [`core::alloc::Allocator`] или [`DryAllocator`],
    /// которые не принимают на вход флагов отображений страниц.
    ///
    /// Возвращает ошибку:
    ///   - [`Error::PermissionDenied`] если запрошенные флаги
    ///     не допускаются этим аллокатором.
    fn set_flags(
        &mut self,
        flags: PageTableFlags,
    ) -> Result<()>;

    /// Выделяет новый блок подряд идущих виртуальных страниц.
    /// Достаточный для хранения объекта, размер и выравнивание которого описывается `layout`.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// - Если выделить заданный размер виртуальной памяти не удалось,
    ///   возвращает ошибку [`Error::NoPage`].
    fn reserve(
        &mut self,
        layout: Layout,
    ) -> Result<Block<Page>>;

    /// Выделяет блок `block` виртуальной памяти.
    /// Ни выделения физической памяти, ни создания отображения станиц, не происходит.
    ///
    /// - Возвращает ошибку [`Error::NoPage`],
    ///   если выделить заданный блок виртуальной памяти не удалось.
    ///   То есть, если он содержит хотя бы одну уже занятую страницу.
    fn reserve_fixed(
        &mut self,
        block: Block<Page>,
    ) -> Result<()>;

    /// Освобождает блок виртуальных страниц `block`.
    ///
    /// # Safety
    ///
    /// - `block` должен был быть ранее выделен с помощью [`BigAllocator::reserve()`]
    ///   или [`BigAllocator::reserve_fixed()`].
    /// - Отображения этих станиц в физическую память уже не должно быть.
    /// - Вызывающий код должен гарантировать,
    ///   что инварианты управления памятью в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в `block`.
    unsafe fn unreserve(
        &mut self,
        block: Block<Page>,
    ) -> Result<()>;

    /// Уменьшает ранее зарезервированный блок виртуальных страниц `old_block`
    /// до его подблока `sub_block`.
    /// Возвращает ошибку [`Error::InvalidArgument`], если `sub_block`
    /// не содержится в `old_block` целиком.
    ///
    /// # Safety
    ///
    /// - `old_block` должен был быть ранее выделен с помощью [`BigAllocator::reserve()`]
    ///   или [`BigAllocator::reserve_fixed()`].
    /// - Для станиц `old_block`, не попадающих в `sub_block`,
    ///   отображения в физическую память уже не должно быть.
    /// - Вызывающий код должен гарантировать,
    ///   что инварианты управления памятью в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в освобождаемые страницы.
    unsafe fn rereserve(
        &mut self,
        old_block: Block<Page>,
        sub_block: Block<Page>,
    ) -> Result<()>;

    /// Выделяет нужное количество физических фреймов
    /// и отображает в них заданный блок виртуальных страниц `block` с флагами `flags`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::NoFrame`] если свободных физических фреймов не осталось.
    ///   - [`Error::PermissionDenied`] если запрошенные флаги
    ///     не допускаются этим аллокатором.
    ///
    /// # Safety
    ///
    /// - `block` должен был быть ранее выделен с помощью [`BigAllocator::reserve()`]
    ///   или [`BigAllocator::reserve_fixed()`].
    /// - Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    ///   не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в `block`.
    unsafe fn map(
        &mut self,
        block: Block<Page>,
        flags: PageTableFlags,
    ) -> Result<()>;

    /// Удаляет отображение заданного блока виртуальных страниц `block`.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    /// После работы [`BigAllocator::unmap()`] виртуальные адреса `block`
    /// становятся недоступны.
    ///
    /// # Safety
    ///
    /// - `block` должен был быть ранее отображён с помощью [`BigAllocator::map()`]
    ///   или [`BigAllocator::copy_mapping()`].
    /// - Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    ///   не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в `block`.
    unsafe fn unmap(
        &mut self,
        block: Block<Page>,
    ) -> Result<()>;

    /// Копирует отображение физических фреймов из `old_block` в `new_block`.
    /// Если изначально `new_block` содержал отображённые страницы,
    /// их отображение удаляется.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    /// А содержимое памяти, которое ранее было доступно через `old_block`,
    /// становится доступным и через `new_block`.
    ///
    /// Параметр `flags` задаёт флаги доступа к страницам `new_block`:
    ///   - [`None`] --- использовать те же флаги, что и в `old_block`,
    ///     индивидуально для каждой страницы.
    ///   - [`Some`] --- использовать для всех страниц флаги `flags`.
    ///
    /// В случае совпадения `old_block` и `new_block`, необходимо задать флаги `flags`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если `old_block` и `new_block` имеют разный размер.
    ///   - [`Error::InvalidArgument`] если `old_block` и `new_block` пересекаются, но
    ///     не совпадают. Или совпадают и при этом `flags == None`.
    ///     (Ситуация смены флагов на одном и том же блоке является единственной допустимой
    ///     при пересечении `old_block` и `new_block`).
    ///   - [`Error::PermissionDenied`] если запрошенные флаги
    ///     не допускаются этим аллокатором.
    ///
    /// # Safety
    ///
    /// - `old_block` должен был быть ранее отображён с помощью [`BigAllocator::map()`]
    ///   или [`BigAllocator::copy_mapping()`].
    /// - `new_block` должен был быть ранее выделен
    ///   с помощью [`BigAllocator::reserve()`] или [`BigAllocator::reserve_fixed()`].
    /// - Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    ///   не будут нарушены.
    ///   В частности, не возникнет неуникальных изменяемых ссылок.
    unsafe fn copy_mapping(
        &mut self,
        old_block: Block<Page>,
        new_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()>;

    // ANCHOR: grow
    /// Аналогично [`Allocator::grow()`]
    /// увеличивает ранее отображённый блок виртуальных страниц `old_block`.
    ///
    /// При `old_block.size() == 0` выделяет полный новый блок.
    /// Метод [`BigAllocator::grow(Block::default(), layout, flags)`] может быть использован
    /// для выделения нового блока вместо пары вызовов
    /// [`BigAllocator::reserve()`] и [`BigAllocator::map()`].
    /// А вызов [`BigAllocator::shrink(block, Page::layout_array(0))`] может быть использован
    /// для полного освобождения блока вместо пары вызовов
    /// [`BigAllocator::unmap()`] и [`BigAllocator::unreserve()`].
    ///
    /// # Safety
    ///
    /// - `old_block` должен был быть пустой или должен быть ранее отображён с помощью
    ///   [`BigAllocator::map()`] или [`BigAllocator::copy_mapping()`].
    /// - Вызывающий код должен гарантировать,
    ///   что инварианты управления памятью в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в освобождаемые страницы.
    ///
    /// # Panics
    ///
    /// Паникует, если `new_layout.size() < old_block.size()`.
    unsafe fn grow(
        &mut self,
        old_block: Block<Page>,
        new_layout: Layout,
        flags: PageTableFlags,
    ) -> Result<Block<Page>> {
        // ANCHOR_END: grow
        assert!(new_layout.size() >= old_block.size());
        
        if old_block.is_empty() {
            let new_block = self.reserve(new_layout)?;
            unsafe {
                self.map(new_block, flags)?;
            }
            return Ok(new_block);
        }
        
        let new_page_count = (new_layout.size() + Page::SIZE - 1) / Page::SIZE;
        let new_block = Block::<Page>::from_index(
            old_block.start(),
            old_block.start() + new_page_count,
        )?;
        
        if old_block.count() == new_block.count() {
            return Ok(old_block);
        }
        
        let additional_start = old_block.end();
        let additional_end = new_block.end();
        let additional_block = Block::from_index(additional_start, additional_end)?;
        
        if self.reserve_fixed(additional_block).is_ok() {
            unsafe {
                self.map(additional_block, flags)?;
            }
            return Ok(new_block);
        }
        
        let allocated_block = self.reserve(new_layout)?;
        let copy_count = old_block.count().min(allocated_block.count());
        let copy_block = Block::from_index(
            allocated_block.start(),
            allocated_block.start() + copy_count,
        )?;
        
        unsafe {
            self.copy_mapping(old_block, copy_block, None)?;
        }
        
        if allocated_block.count() > old_block.count() {
            let new_additional_start = allocated_block.start() + old_block.count();
            let new_additional_end = allocated_block.end();
            let new_additional_block = Block::from_index(new_additional_start, new_additional_end)?;
            
            unsafe {
                self.map(new_additional_block, flags)?;
            }
        }
        
        unsafe {
            self.unmap(old_block)?;
            self.unreserve(old_block)?;
        }
        
        Ok(allocated_block)
    }

    // ANCHOR: shrink
    /// Аналогично [`Allocator::shrink()`]
    /// уменьшает ранее отображённый блок виртуальных страниц `old_block`.
    /// Возвращает:
    ///   - Подблок, начинающийся с того же адреса, что и `old_block`,
    ///     если начало `old_block` удовлетворяет новому выравниванию `layout.align()`.
    ///     Для станиц в конце `old_block`, не попадающих в результирующий подблок,
    ///     вызывается [`BigAllocator::unreserve()`].
    ///   - Новый блок, не пересекающийся с `old_block`,
    ///     если начало `old_block` не удовлетворяет новому выравниванию `layout.align()`.
    ///     Для `old_block` вызывается [`BigAllocator::unreserve()`].
    ///
    /// Флаги отображения страниц нового блока постранично совпадают с флагами,
    /// с которыми были отображены страницы соответствующего префикса старого блока.
    ///
    /// При `new_layout.size() == 0` полностью освобождает `old_block`.
    /// Метод [`BigAllocator::grow(Block::default(), layout, flags)`] может быть использован
    /// для выделения нового блока вместо пары вызовов
    /// [`BigAllocator::reserve()`] и [`BigAllocator::map()`].
    /// А вызов [`BigAllocator::shrink(block, Page::layout_array(0))`] может быть использован
    /// для полного освобождения блока вместо пары вызовов
    /// [`BigAllocator::unmap()`] и [`BigAllocator::unreserve()`].
    ///
    /// # Safety
    ///
    /// - `old_block` должен был быть ранее отображён с помощью [`BigAllocator::map()`]
    ///   или [`BigAllocator::copy_mapping()`].
    /// - Вызывающий код должен гарантировать,
    ///   что инварианты управления памятью в Rust'е не будут нарушены.
    ///   В частности, не осталось ссылок, которые ведут в освобождаемые страницы.
    ///
    /// # Panics
    ///
    /// Паникует, если `new_layout.size() > old_block.size()`.
    unsafe fn shrink(
        &mut self,
        old_block: Block<Page>,
        new_layout: Layout,
    ) -> Result<Block<Page>> {
        // ANCHOR_END: shrink
        // TODO: your code here.
        unimplemented!();
    }
}

unsafe impl<T: BigAllocator> DryAllocator for T {
    fn dry_allocate(
        &mut self,
        layout: Layout,
        initialize: Initialize,
    ) -> Result<NonNull<[u8]>> {
        let block = self.reserve(layout)?;
        unsafe {
            self.map(block, self.flags())?;
        }
        unsafe {
            initialize_block(block, initialize)?;
        }
        unsafe { block.try_into_non_null_slice() }
    }

    unsafe fn dry_deallocate(
        &mut self,
        ptr: NonNull<u8>,
        layout: Layout,
    ) {
        let block = try_into_block(ptr, layout).expect("invalid deallocation");
        unsafe {
            self.unmap(block).expect("failed to unmap block");
        }
        unsafe {
            self.unreserve(block).expect("failed to unreserve block");
        }
    }

    unsafe fn dry_grow(
        &mut self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
        initialize: Initialize,
    ) -> Result<NonNull<[u8]>> {
        let old_block = try_into_block(ptr, old_layout)?;
        let new_block = try_into_block(ptr, new_layout)?;
        if old_block.count() == new_block.count() {
            return unsafe { new_block.try_into_non_null_slice() };
        }
        let additional_start = old_block.end();
        let additional_end = new_block.end();
        let additional_block = Block::from_index(additional_start, additional_end)?;
        if self.reserve_fixed(additional_block).is_ok() {
            unsafe {
                self.map(additional_block, self.flags())?;
                initialize_block(additional_block, initialize)?;
            }
            return unsafe { new_block.try_into_non_null_slice() };
        }
        let allocated_block = self.reserve(new_layout)?;
        let copy_count = old_block.count().min(allocated_block.count());
        let copy_block = Block::from_index(allocated_block.start(), allocated_block.start() + copy_count)?;
        
        unsafe {
            self.copy_mapping(old_block, copy_block, None)?;
        }
        if allocated_block.count() > old_block.count() {
            let new_additional_start = allocated_block.start() + old_block.count();
            let new_additional_end = allocated_block.end();
            let new_additional_block = Block::from_index(new_additional_start, new_additional_end)?;
            
            unsafe {
                self.map(new_additional_block, self.flags())?;
                initialize_block(new_additional_block, initialize)?;
            }
        }
        unsafe {
            self.unmap(old_block)?;
            self.unreserve(old_block)?;
        }
        
        unsafe { allocated_block.try_into_non_null_slice() }
    }

    unsafe fn dry_shrink(
        &mut self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>> {
        let old_block = try_into_block(ptr, old_layout)?;
        let new_block = try_into_block(ptr, new_layout)?;
        if old_block.count() == new_block.count() {
            return unsafe { new_block.try_into_non_null_slice() };
        }
        let tail_start = new_block.end();
        let tail_end = old_block.end();
        let tail_block = Block::from_index(tail_start, tail_end)?;
        unsafe {
            self.unmap(tail_block)?;
            self.rereserve(old_block, new_block)?;
        }
        
        unsafe { new_block.try_into_non_null_slice() }
    }
}

/// Типаж, который позволяет захватывать [`BigAllocator`].
pub trait BigAllocatorGuard {
    /// Захватывает [`BigAllocator`] и возвращает его,
    /// возможно обёрнутым во вспомогательный объект.
    ///
    /// Если это требуется в реализации,
    /// может захватывать блокировку на возвращаемый [`BigAllocator`].
    /// В этом случае, блокировка освобождается при вызове [`Drop::drop()`]
    /// для возвращённого объекта.
    fn get(&self) -> impl BigAllocator;
}

/// Преобразует `ptr` и `layout` в соответствующий блок страниц.
///
/// Возвращает ошибки:
///   - [`Error::InvalidAlignment`], если `ptr` не выровнен на границу страницы.
///   - [`Error::Overflow`] или [`Error::InvalidArgument`], если получающийся блок
///     пересекает границу одной из половин адресного пространства.
fn try_into_block(
    ptr: NonNull<u8>,
    layout: Layout,
) -> Result<Block<Page>> {
    use crate::error::Error::InvalidAlignment;
    
    let virt = Virt::from_ptr(ptr.as_ptr());
    if virt.into_usize() % Page::SIZE_OF != 0 {
        return Err(InvalidAlignment);
    }
    let start_virt = virt;
    let end_virt = (virt + layout.size())?;
    let block_virt = Block::<Virt>::new(start_virt, end_virt)?;
    let page_block = block_virt.enclosing();
    
    Ok(page_block)
}

/// Инициализирует `block` так, как предписывает `initialize`.
///
/// # Safety
///
/// - `block` должен быть отображён в память.
/// - Вызывающий код должен гарантировать,
///   что инварианты управления памятью в Rust'е не будут нарушены.
///   В частности, нет ссылок, которые ведут в `block`.
unsafe fn initialize_block(
    block: Block<Page>,
    initialize: Initialize,
) -> Result<()> {
    if initialize == Initialize::Zero {
        let slice = unsafe { block.try_into_mut_slice::<MaybeUninit<u8>>()? };
        for byte in slice.iter_mut() {
            *byte = MaybeUninit::zeroed();
        }
    }
    Ok(())
}
