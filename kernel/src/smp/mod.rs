/// Обвязка внешней библиотеки [`acpi`], которая используется для получения
/// информации о конфигурации оборудования из таблиц, предоставляемых BIOS.
mod acpi_info;

/// Код инициализации Application Processors.
mod ap_init;

/// Код для работы с вектором структур [`Cpu`],
/// каждая из которых принадлежит своему процессору системы.
mod cpu;

/// Код работы с
/// local [APIC](https://en.wikipedia.org/wiki/Advanced_Programmable_Interrupt_Controller).
mod local_apic;

use alloc::vec::Vec;
use core::cmp;

use lazy_static::lazy_static;

use ku::sync::spinlock::Spinlock;

use crate::{
    Subsystems,
    error::{
        Error::Unimplemented,
        Result,
    },
    log::{
        info,
        warn,
    },
    memory::Phys2Virt,
    time,
};

use acpi_info::AcpiInfo;
use ap_init::SavedMemory;

pub(crate) use cpu::{
    Cpu,
    KERNEL_RSP_OFFSET_IN_CPU,
};
pub(crate) use local_apic::{
    CpuId,
    LocalApic,
};

/// Зануляет регистр
/// [`GS`](https://wiki.osdev.org/CPU_Registers_x86-64#FS.base.2C_GS.base)
/// текущего CPU, чтобы отловить попытки его использования до инициализации
/// методом [`Cpu::set_gs()`].
pub(super) fn preinit() {
    Cpu::clear_gs();
}

/// Инициализация симметричной многопроцессорности
/// ([Symmetric multiprocessing](https://en.wikipedia.org/wiki/Symmetric_multiprocessing), SMP).
pub(super) fn init(
    phys2virt: Phys2Virt,
    subsystems: Subsystems,
) {
    let timer = time::timer();

    if let Err(error) = init_smp(phys2virt, subsystems) {
        warn!(?error, duration = %timer.elapsed(), "SMP init");
    } else {
        info!(duration = %timer.elapsed(), "SMP init");
    }
}

/// Инициализация симметричной многопроцессорности
/// ([Symmetric multiprocessing](https://en.wikipedia.org/wiki/Symmetric_multiprocessing), SMP).
/// Внутренняя функция, которая выполняет всю работу.
fn init_smp(
    phys2virt: Phys2Virt,
    subsystems: Subsystems,
) -> Result<()> {
    if !subsystems.contains(Subsystems::LOCAL_APIC) {
        return Err(Unimplemented);
    }

    let acpi_info = AcpiInfo::new(phys2virt)?;
    let local_apic_address = acpi_info.local_apic_address();

    LocalApic::map(local_apic_address)?;
    LocalApic::init();

    let bootstrap_processor = acpi_info.bsp_id();
    let current_cpu = LocalApic::id();
    if bootstrap_processor != current_cpu {
        warn!(
            bootstrap_processor,
            current_cpu, "ACPI Bootstrap Processor is not the current processor",
        );
        return Err(Unimplemented);
    }

    let max_cpu_id = cmp::max(
        bootstrap_processor,
        *acpi_info.ap_ids().iter().max().unwrap_or(&bootstrap_processor),
    );
    let cpu_count = usize::from(max_cpu_id) + 1;
    info!(cpu = current_cpu, cpu_count, %local_apic_address, "Local APIC init");

    if !subsystems.contains(Subsystems::CPUS) {
        return Err(Unimplemented);
    }

    let mut cpus = cpu::init(cpu_count, current_cpu)?;

    assert_eq!(cpus.len(), cpu_count);

    info!(cpu = current_cpu, "Bootstrap Processor");

    if subsystems.contains(Subsystems::BOOT_APS) {
        for id in acpi_info.ap_ids() {
            ap_init::boot_ap(phys2virt, &mut cpus[usize::from(*id)])?;
        }
    }

    *CPUS.lock() = cpus;

    Ok(())
}

lazy_static! {
    /// Структуры [`Cpu`] для всех процессоров в системе.
    static ref CPUS: Spinlock<Vec<Cpu>> = Spinlock::new(Vec::<Cpu>::default());
}

#[doc(hidden)]
pub mod test_scaffolding {
    use crate::{
        Subsystems,
        error::Result,
        memory::{
            Block,
            Phys2Virt,
            Virt,
        },
    };

    use super::CPUS;

    pub use super::{
        cpu::test_scaffolding::*,
        local_apic::test_scaffolding::*,
    };

    pub fn cpu_count() -> usize {
        CPUS.lock().len()
    }

    pub fn init_smp(
        phys2virt: Phys2Virt,
        subsystems: Subsystems,
    ) -> Result<()> {
        super::init_smp(phys2virt, subsystems)
    }

    pub fn kernel_stack_zones(cpu: usize) -> (Block<Virt>, Block<Virt>) {
        CPUS.lock()[cpu].kernel_stack().zones()
    }
}
