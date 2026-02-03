use crate::{
    error::Result,
    memory::{
        Block,
        Page,
        mmu::PageTableFlags,
    },
};

use super::BigAllocator;

// Used in docs.
#[allow(unused)]
use crate::error::Error;

/// Позволяет копировать отображение блока страниц между двумя [`BigAllocator`].
///
/// Требуется, когда нужно использовать [`BigAllocator`]
/// либо для текущего адресного пространства, в котором работает код,
/// либо вообще говоря для другого адресного пространства.
///
/// Например:
///   - Из одного адресного пространства загрузить в другое код процесса.
///   - Из одного адресного пространства создать разделяемую
///     с другим адресным пространством область памяти.
pub trait BigAllocatorPair {
    /// Возвращает [`BigAllocator`], который соответствует назначению
    /// в методе [`BigAllocatorPair::copy_mapping()`].
    fn dst(&mut self) -> impl BigAllocator;

    /// Возвращает [`BigAllocator`], который соответствует источнику
    /// в методе [`BigAllocatorPair::copy_mapping()`].
    fn src(&mut self) -> impl BigAllocator;

    /// Возвращает `true`, если `src` и `dst` --- это один и тот же [`BigAllocator`].
    fn is_same(&self) -> bool;

    /// Копирует отображение физических фреймов из `src()`/`src_block` в `dst()`/`dst_block`.
    /// Если изначально `dst_block` содержал отображённые страницы,
    /// их отображение удаляется.
    /// Физические фреймы, на которые не осталось других ссылок, освобождаются.
    /// А содержимое памяти, которое ранее было доступно через `src_block`,
    /// становится доступным и через `dst_block`.
    ///
    /// Параметр `flags` задаёт флаги доступа к страницам `dst_block`:
    ///   - [`None`] --- использовать те же флаги, что и в `src_block`,
    ///     индивидуально для каждой страницы.
    ///   - [`Some`] --- использовать для всех страниц флаги `flags`.
    ///
    /// В случае совпадения `src_block` и `dst_block`, необходимо задать флаги `flags`.
    ///
    /// Возвращает ошибки:
    ///   - [`Error::InvalidArgument`] если `src_block` и `dst_block` имеют разный размер.
    ///   - [`Error::InvalidArgument`] если `src() == dst()`, и `src_block` и `dst_block`
    ///     пересекаются, но не совпадают. Или `src() == dst()`, и `src_block` и `dst_block`
    ///     совпадают и при этом `flags == None`.
    ///     (Ситуация смены флагов на одном и том же блоке одного и того же [`BigAllocator`]
    ///     является единственной допустимой при пересечении `src_block` и `dst_block` внутри
    ///     одного и того же [`BigAllocator`]).
    ///   - [`Error::PermissionDenied`] если флаги хотя бы одной страницы назначения
    ///     не допускаются аллокатором `dst()`.
    ///
    /// # Safety
    ///
    /// - `src_block` и `dst_block` должны был быть ранее выделены
    ///   с помощью [`BigAllocator::reserve()`] или [`BigAllocator::reserve_fixed()`].
    /// - Вызывающий код должен гарантировать, что инварианты управления памятью в Rust'е
    ///   не будут нарушены.
    ///   В частности, не возникнет неуникальных изменяемых ссылок.
    unsafe fn copy_mapping(
        &mut self,
        src_block: Block<Page>,
        dst_block: Block<Page>,
        flags: Option<PageTableFlags>,
    ) -> Result<()>;
}
