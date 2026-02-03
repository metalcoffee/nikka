use lazy_static::lazy_static;
use x86_64::structures::tss::TaskStateSegment;

use super::stack::EXCEPTION_STACKS;

lazy_static! {
    /// Сегмент состояния задачи
    /// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS)
    pub(super) static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        let stacks = EXCEPTION_STACKS.lock();

        tss.interrupt_stack_table[usize::from(DOUBLE_FAULT_IST_INDEX)] =
            stacks.double_fault_rsp().into();
        tss.interrupt_stack_table[usize::from(PAGE_FAULT_IST_INDEX)] =
            stacks.page_fault_rsp().into();

        tss
    };
}

/// Индекс в сегменте состояния задачи
/// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS)
/// стека, выделенного для обработки
/// [Double Fault](https://en.wikipedia.org/wiki/Double_fault).
pub(crate) const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Индекс в сегменте состояния задачи
/// ([Task State Segment](https://en.wikipedia.org/wiki/Task_state_segment), TSS)
/// стека, выделенного для обработки
/// [Page Fault](https://en.wikipedia.org/wiki/Page_fault).
pub(crate) const PAGE_FAULT_IST_INDEX: u16 = 1;
