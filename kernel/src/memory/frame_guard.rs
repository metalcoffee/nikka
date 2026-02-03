use core::mem;

use derive_more::{
    Deref,
    Display,
};

use ku::memory::mmu::{
    PageTableEntry,
    PageTableFlags,
};

use crate::error::Result;

use super::{
    FRAME_ALLOCATOR,
    frage::Frame,
};

// Used in docs.
#[allow(unused)]
use {
    super::FrameAllocator,
    ku::error::Error,
};

#[allow(rustdoc::private_intra_doc_links)]
/// RAII для операции выделения одного [`Frame`].
///
/// Вызывает [`FrameAllocator::deallocate()`] для [frame][Frame] при
/// [`FrameGuard::drop()`], если [frame][Frame]
/// не был забран ранее с помощью [`FrameGuard::take()`].
///
/// # Examples
///
/// ## Автоматическое освобождение фрейма в случае ошибки
/// ```ignore
/// # fn fallible_initialization(_frame: Frame) -> Result<()> {
/// #     Ok(())
/// # }
/// #
/// fn get_initialized_frame() -> Result<Frame> {
///     let frame = FrameGuard::allocate()?;
///
///     // Благодаря тому, что FrameGuard реализует типаж Deref<Target = Frame>,
///     // принадлежащий ему Frame доступен через разыменование.
///     // В случае, если этот вызов возвращает Err(...), утечки не происходит и Frame освобождается
///     // автоматически.
///     fallible_initialization(*frame)?;
///     Ok(frame.take())
/// }
/// ```
#[derive(Debug, Deref, Display, Eq, Ord, PartialEq, PartialOrd)]
#[display("{}", frame)]
#[must_use]
pub struct FrameGuard {
    /// Фрейм, защищаемый от утечки.
    frame: Frame,
}

impl FrameGuard {
    /// Создает новый [`FrameGuard`] для данного [`frame`][Frame].
    ///
    /// Ограничение видимости `pub(super)` требуется для того, чтобы внешние
    /// (по отношению к `memory`) модули не могли создавать [`FrameGuard`] самостоятельно.
    /// И тем самым обойти контроль за утечками фреймов.
    pub(super) fn new(frame: Frame) -> Self {
        Self { frame }
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Создает новый [`FrameGuard`] от выражения
    /// [`FRAME_ALLOCATOR.lock().allocate()`][FrameAllocator::allocate].
    ///
    /// # Errors
    ///
    /// - [`Error::NoFrame`] --- свободных физических фреймов не осталось.
    pub fn allocate() -> Result<Self> {
        FRAME_ALLOCATOR.lock().allocate()
    }

    /// Забирает из [`pte`][PageTableEntry] физический фрейм, на который она указывает.
    /// Возвращает этот фрейм, обёрнутый во [`FrameGuard`].
    /// Сама [`pte`][PageTableEntry] очищается.
    ///
    /// # Errors
    ///
    /// - [`Error::NoPage`] --- эта [`pte`][PageTableEntry] не используется,
    ///   то есть, сброшен бит [`PageTableFlags::PRESENT`].
    pub fn load(pte: &mut PageTableEntry) -> Result<Self> {
        Ok(Self::new(pte.take()?))
    }

    #[allow(rustdoc::private_intra_doc_links)]
    /// Создает новый [`FrameGuard`] от выражения
    /// [`FRAME_ALLOCATOR.lock().reference(frame)`][FrameAllocator::reference].
    pub fn reference(frame: Frame) -> Self {
        FRAME_ALLOCATOR.lock().reference(frame)
    }

    /// Записывает [`Frame`] из данного [`FrameGuard`] в
    /// [`pte`][PageTableEntry] с заданными флагами [`flags`][PageTableFlags].
    pub fn store(
        self,
        pte: &mut PageTableEntry,
        flags: PageTableFlags,
    ) {
        pte.set_frame(self.take(), flags);
    }

    /// Забирает [`Frame`] из данного [`FrameGuard`].
    pub(super) fn take(self) -> Frame {
        let frame = self.frame;
        mem::forget(self);
        frame
    }
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        FRAME_ALLOCATOR.lock().deallocate(self.frame);
    }
}
