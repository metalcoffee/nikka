use core::{
    cmp,
    mem,
    ops::Range,
};

use bootloader::bootinfo::{
    MemoryMap,
    MemoryRegionType,
};
use duplicate::duplicate_item;
use itertools::Itertools;
use lazy_static::lazy_static;

use ku::sync::Spinlock;

use crate::{
    error::{
        Error::NoFrame,
        Result,
    },
    log::{
        debug,
        error,
        info,
        trace,
    },
    time,
};

use super::{
    BASE_ADDRESS_SPACE,
    Block,
    FrameGuard,
    KERNEL_RW,
    Size,
    frage::Frame,
    size,
};

// Used in docs.
#[allow(unused)]
use {
    super::Mapping,
    crate::error::Error,
};

/// Информация об одном физическом фрейме.
#[derive(Clone, Copy, Debug, Default)]
enum FrameInfo {
    /// Фрейм не доступен --- либо находится за пределами физической памяти,
    /// либо зарезервирован аппаратурой, загрузчиком или BIOS.
    #[default]
    Absent,

    /// Фрейм свободен.
    Free {
        /// Номер следующего свободного фрейма.
        next_free: Option<usize>,
    },

    /// Фрейм занят.
    Used {
        /// Количество ссылок на этот фрейм.
        /// Например, из одного или разных страничных отображений [`Mapping`].
        reference_count: usize,
    },
}

/// Инициализирует аллокатор [`FrameAllocator`] по информации из `memory_map`.
/// Использует для него временное хранилище метаданных.
#[allow(static_mut_refs)]
pub(super) fn init(memory_map: &MemoryMap) -> FrameAllocator {
    // ANCHOR: log_tsc
    let timer = time::timer();
    let frame_allocator = FrameAllocator::new(unsafe { &mut BOOT_FRAME_INFO.0 }, memory_map);
    info!(
        free_frame_count = frame_allocator.count(),
        duration = %timer.elapsed(),
        "frame allocator temporary storage init",
    );
    // ANCHOR_END: log_tsc
    frame_allocator
}

// ANCHOR: fn_resize
/// Изменяет размер метаданных аллокатора [`FrameAllocator`] так,
/// чтобы он мог хранить информацию про
/// всю доступную в машине физическую память --- `physical_memory`.
///
/// Уже имеющиеся в [`FrameAllocator`] метаданные переносит
/// из временного хранилища в новое и освобождает временное хранилище.
/// Ещё не заполненные метаданные заполняет по информации из `memory_map`.
pub(super) fn resize(
    physical_memory: Block<Frame>,
    memory_map: &MemoryMap,
) {
    // ANCHOR_END: fn_resize
    let timer = time::timer();
    
    // Calculate the number of frames we need to track
    let frame_count = physical_memory.count();
    
    // Allocate new storage for frame metadata (before locking FRAME_ALLOCATOR)
    let new_frame_info = BASE_ADDRESS_SPACE
        .lock()
        .map_slice(frame_count, KERNEL_RW, FrameInfo::default)
        .expect("failed to allocate frame_info");
    
    // Resize the frame allocator with the new storage
    let old_frame_info = {
        let mut frame_allocator = FRAME_ALLOCATOR.lock();
        frame_allocator.resize(new_frame_info, memory_map)
    };
    
    // Free the old temporary storage
    unsafe {
        BASE_ADDRESS_SPACE
            .lock()
            .unmap_slice(old_frame_info)
            .expect("failed to free old frame_info");
    }
    
    let free_frame_count = FRAME_ALLOCATOR.lock().count();
    
    info!(
        free_frame_count,
        duration = %timer.elapsed(),
        "frame allocator resize",
    );
}

/// Основной аллокатор физических фреймов.
#[derive(Default)]
pub struct FrameAllocator {
    /// Вспомогательная запись, которая используется при запросе номера фрейма,
    /// выходящего за границы [`FrameAllocator::frame_info`].
    absent: FrameInfo,

    /// Был ли доступ за границы [`FrameAllocator::frame_info`].
    access_beyond_frame_info: bool,

    /// Информация про все доступные физические фреймы.
    frame_info: &'static mut [FrameInfo],

    /// Количество свободных физических фреймов.
    free_count: usize,

    /// Голова интрузивного списка номеров свободных физических фреймов.
    free_frame: Option<usize>,
}

impl FrameAllocator {
    /// Инициализирует аллокатор [`FrameAllocator`] по информации из `memory_map`.
    fn new(
        frame_info: &'static mut [FrameInfo],
        memory_map: &MemoryMap,
    ) -> Self {
        let mut frame_allocator = Self {
            absent: FrameInfo::Absent,
            access_beyond_frame_info: false,
            frame_info,
            free_count: 0,
            free_frame: None,
        };

        let frame_count = frame_allocator.frame_info.len();

        frame_allocator.init_frame_info(memory_map, 0 .. frame_count);

        debug!(
            frame_count,
            physical_memory = %Size::new::<Frame>(frame_count),
            "frame allocator temporary storage",
        );

        frame_allocator
    }

    /// Возвращает количество свободных физических фреймов у аллокатора.
    pub fn count(&self) -> usize {
        self.free_count
    }

    // ANCHOR: allocate
    /// Выделяет ровно один физический фрейм.
    /// Возвращает [`FrameGuard`], владеющий ссылкой на этот фрейм.
    ///
    /// Если свободных физических фреймов не осталось,
    /// возвращает ошибку [`Error::NoFrame`].
    pub fn allocate(&mut self) -> Result<FrameGuard> {
        // ANCHOR_END: allocate
        if self.free_count == 0 || self.free_frame.is_none() {
            return Err(NoFrame);
        }
        let frame_index = self.free_frame.unwrap();
        let frame_info = self.frame_info[frame_index];
        match frame_info {
            FrameInfo::Free { next_free } => {
                self.free_frame = next_free;
                self.free_count -= 1;
                self.frame_info[frame_index] = FrameInfo::Used {
                    reference_count: 1,
                };
                let frame = Frame::from_index(frame_index).expect("err");
                Ok(FrameGuard::new(frame))
            }
            _ => {
                panic!("err");
            }
        }
    }

    // ANCHOR: deallocate
    /// Уменьшает на единицу счётчик использований заданного физического фрейма `frame`.
    /// Физический фрейм освобождается, если на него не осталось других ссылок.
    ///
    /// # Panics
    ///
    /// Паникует, если фрейм свободен.
    pub fn deallocate(
        &mut self,
        frame: Frame,
    ) {
        // ANCHOR_END: deallocate
        let frame_index = frame.index();
        if frame_index >= self.frame_info.len() {
            return;
        }
        let frame_info = &mut self.frame_info[frame_index];
        
        match frame_info {
            FrameInfo::Absent => {
                return;
            }
            FrameInfo::Free { .. } => {
                panic!("err");
            }
            FrameInfo::Used { reference_count } => {
                *reference_count -= 1;
                if *reference_count == 0 {
                    *frame_info = FrameInfo::Free {
                        next_free: self.free_frame,
                    };
                    self.free_frame = Some(frame_index);
                    self.free_count += 1;
                }
            }
        }
    }

    // ANCHOR: reference
    /// Увеличивает на единицу счётчик использований заданного физического фрейма `frame`.
    /// Возвращает [`FrameGuard`], владеющий новой ссылкой на этот фрейм.
    ///
    /// # Panics
    ///
    /// Паникует, если фрейм свободен.
    pub fn reference(
        &mut self,
        frame: Frame,
    ) -> FrameGuard {
        // ANCHOR_END: reference
        let frame_index = frame.index();
        if frame_index >= self.frame_info.len() {
            return FrameGuard::new(frame);
        }
        let frame_info = &mut self.frame_info[frame_index];
        
        match frame_info {
            FrameInfo::Absent => {
                return FrameGuard::new(frame);
            }
            FrameInfo::Free { .. } => {
                panic!("err");
            }
            FrameInfo::Used { reference_count } => {
                *reference_count += 1;
                return FrameGuard::new(frame);
            }
        }
    }

    /// Возвращает количество ссылок на `frame`.
    ///
    /// - Если `frame` свободен, возвращается `0`.
    /// - Если он отсутствует или зарезервирован --- [`Error::NoFrame`].
    pub fn reference_count(
        &self,
        frame: Frame,
    ) -> Result<usize> {
        match *self.frame_info(frame) {
            FrameInfo::Absent => Err(NoFrame),
            FrameInfo::Free { .. } => Ok(0),
            FrameInfo::Used { reference_count } => Ok(reference_count),
        }
    }

    /// Проверяет, что заданный физический фрейм уже был выделен.
    pub fn is_used(
        &self,
        frame: Frame,
    ) -> bool {
        !matches!(self.frame_info(frame), FrameInfo::Free { .. })
    }

    // ANCHOR: init_frame_info
    /// Инициализирует метаданные аллокатора в диапазоне номеров
    /// физических фреймов `frames` по информации из `memory_map`.
    fn init_frame_info(
        &mut self,
        memory_map: &MemoryMap,
        frames: Range<usize>,
    ) {
        // ANCHOR_END: init_frame_info
        for region in memory_map.iter() {
            let region_frames = Block::<Frame>::from_index_u64(
                region.range.start_frame_number,
                region.range.end_frame_number,
            ).expect("err");
            let intersection = region_frames.intersection(
                Block::from_index(frames.start, frames.end)
                    .expect("err")
            );
            if intersection.is_empty() {
                continue;
            }
            match region.region_type {
                MemoryRegionType::Usable => {
                    for frame in intersection {
                        let frame_index = frame.index();
                        if frame_index < self.frame_info.len() {
                            self.frame_info[frame_index] = FrameInfo::Free {
                                next_free: self.free_frame,
                            };
                            self.free_frame = Some(frame_index);
                            self.free_count += 1;
                        }
                    }
                }
                MemoryRegionType::Kernel |
                MemoryRegionType::KernelStack |
                MemoryRegionType::PageTable => {
                    for frame in intersection {
                        let frame_index = frame.index();
                        if frame_index < self.frame_info.len() {
                            self.frame_info[frame_index] = FrameInfo::Used {
                                reference_count: 1,
                            };
                        }
                    }
                }
                _ => {
                }
            }
        }
    }

    // ANCHOR: resize
    /// Переносит уже имеющиеся в [`FrameAllocator`] метаданные
    /// из временного хранилища в новое --- `frame_info`.
    /// Ещё не заполненные метаданные заполняет по информации из `memory_map`.
    /// Переключает [`FrameAllocator::frame_info`] на новое хранилище.
    /// Возвращает старое временное хранилище метаданных.
    fn resize(
        &mut self,
        frame_info: &'static mut [FrameInfo],
        memory_map: &MemoryMap,
    ) -> &'static mut [FrameInfo] {
        // ANCHOR_END: resize
        assert!(
            !self.access_beyond_frame_info,
            "resize() after reference() of a frame beyond frame_info can break FrameAllocator's \
             invariants",
        );

        let old_len = self.frame_info.len();
        let new_len = frame_info.len();

        // Copy existing metadata from old storage to new storage
        frame_info[..old_len].copy_from_slice(&self.frame_info[..old_len]);

        // Initialize remaining metadata for frames beyond the old range
        let old_frame_info = mem::replace(&mut self.frame_info, frame_info);
        
        if new_len > old_len {
            self.init_frame_info(memory_map, old_len .. new_len);
        }

        old_frame_info
    }

    /// Возвращает ссылку на [`FrameInfo`], соответствующую физическому фрейму `frame`.
    ///
    /// Если номер фрейма `frame` попадает в диапазон [`FrameAllocator::frame_info`],
    /// то эта ссылка указывает на соответствующий элемент этого среза.
    ///
    /// Иначе, она указывает на специальную запись [`FrameAllocator::absent`],
    /// содержащую значение [`FrameInfo::Absent`].
    /// В этом случае, вызывающий код не должен менять эту запись.
    /// Кроме того, если запрос был на изменяемую ссылку,
    /// а возвращается [`FrameAllocator::absent`],
    /// дополнительно [`FrameAllocator::access_beyond_frame_info`] выставляется в `true`.
    /// После этого нельзя вызывать [`FrameAllocator::resize()`],
    /// так как это может нарушить инварианты [`FrameAllocator`]:
    /// ```ignore
    /// let frame = frame_beyond_old_frame_info_but_inside_new_frame_info;
    /// let guard = frame_allocator.reference(frame); // no reference increase
    /// frame_allocator.resize(...);
    /// drop(guard); // unbalanced reference decrease
    /// ```
    #[allow(clippy::needless_arbitrary_self_type)]
    #[allow(unused)]
    #[duplicate_item(
        frame_info_getter reference(x) access_beyond_frame_info(x);
        [frame_info] [&x] [];
        [frame_info_mut] [&mut x] [x.access_beyond_frame_info = true;];
    )]
    fn frame_info_getter(
        self: reference([Self]),
        frame: Frame,
    ) -> reference([FrameInfo]) {
        if frame.index() < self.frame_info.len() {
            reference([self.frame_info[frame.index()]])
        } else {
            access_beyond_frame_info([self]);
            reference([self.absent])
        }
    }
}

impl Drop for FrameAllocator {
    fn drop(&mut self) {
        if !self.frame_info.is_empty() {
            panic!("can not drop non-empty FrameAllocator");
        }
    }
}

lazy_static! {
    /// Аллокатор физических фреймов.
    pub static ref FRAME_ALLOCATOR: Spinlock<FrameAllocator> =
        Spinlock::new(FrameAllocator::default());
}

/// Временное хранилище метаданных для [`FrameAllocator`].
#[repr(align(4096))]
struct BootFrameInfo([FrameInfo; BOOT_FRAME_INFO_LEN]);

/// Временное хранилище метаданных для [`FrameAllocator`].
static mut BOOT_FRAME_INFO: BootFrameInfo = BootFrameInfo([FrameInfo::Absent; _]);

/// Количество описателей фреймов [`FrameInfo`] во
/// временном хранилище метаданных для [`FrameAllocator`].
const BOOT_FRAME_INFO_LEN: usize = 64 * Frame::SIZE / mem::size_of::<FrameInfo>();
