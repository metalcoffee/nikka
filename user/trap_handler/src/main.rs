#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use core::{
    hint,
    mem,
    panic::PanicInfo,
    ptr::NonNull,
};

use ku::{
    log::{
        Level,
        info,
    },
    memory::{
        Block,
        Page,
        PageFaultInfo,
        USER_RW,
        Virt,
    },
    process::{
        Info,
        Pid,
        TrapInfo,
    },
};

use lib::{
    entry,
    syscall,
};

entry!(main);

macro_rules! my_assert {
    ($condition:expr $(,)?) => {{
        if !$condition {
            generate_page_fault();
        }
    }};
    ($condition:expr, $message:expr $(,)?) => {{
        my_assert!($condition, $message, 0);
    }};
    ($condition:expr, $message:expr, $value:expr $(,)?) => {{
        if !$condition {
            syscall::log_value(Level::ERROR, $message, $value).unwrap();
            generate_page_fault();
        }
    }};
}

fn main() {
    lib::set_panic_handler(panic_handler);

    let trap_stack = map_pages(0, TRAP_STACK_PAGES);
    my_assert!(syscall::set_trap_handler(Pid::Current, simple_trap_handler, trap_stack).is_ok());
    info!(%trap_stack);

    let block = map_pages(0, 1);
    let slice = fill_block(block, MAIN_VALUE);
    info!(value = slice[0], "stored from main()");

    my_assert!(syscall::unmap(Pid::Current, block).is_ok());

    info!(value = slice[0], "stored from simple_trap_handler()");
    my_assert!(slice[0] == TRAP_HANDLER_VALUE);

    my_assert!(syscall::set_trap_handler(Pid::Current, recursive_trap_handler, trap_stack).is_ok());

    slice.fill(TRASH_VALUE);
    info!(value = slice[0], "stored from main()");
    my_assert!(syscall::unmap(Pid::Current, block).is_ok());

    info!(value = slice[0], "stored from recursive_trap_handler()");
    my_assert!(slice[0] == TRAP_HANDLER_VALUE + MAX_RECURSION_LEVEL);
}

fn recursive_trap_handler(info: &TrapInfo) {
    if let Info::PageFault { address, .. } = info.info() {
        if address == Virt::from_ptr(NonNull::<u8>::dangling().as_ptr()) {
            log(
                Level::ERROR,
                "an assertion has failed, hang up the test ##################################",
                address.into_usize(),
            );
            loop {
                hint::spin_loop();
            }
        }

        let recursion_level = (address.into_usize() % Page::SIZE) / mem::size_of::<usize>();
        log(
            Level::INFO,
            "recursive page fault at level",
            recursion_level,
        );

        if recursion_level > MAX_RECURSION_LEVEL {
            log(
                Level::ERROR,
                "trap handler stack overflow, hang up the test ##################################",
                recursion_level,
            );
            loop {
                hint::spin_loop();
            }
        }

        if recursion_level == MAX_RECURSION_LEVEL {
            let trap_stack = map_pages(0, TRAP_STACK_PAGES);
            log(
                Level::INFO,
                "setting the simple trap handler from the recursive trap handler, new trap_stack \
                 rsp",
                trap_stack.end_address().unwrap().into_usize(),
            );
            my_assert!(
                syscall::set_trap_handler(Pid::Current, simple_trap_handler, trap_stack).is_ok()
            );
        } else {
            let slice = unsafe { address.try_into_mut_slice::<usize>(2) };
            my_assert!(slice.is_ok());
            let slice = slice.unwrap();
            slice[0] = slice[1] + 1;
        }
    } else {
        log(Level::ERROR, "unexpected trap number", info.number());
        my_assert!(false);
    }
}

fn simple_trap_handler(info: &TrapInfo) {
    if let Info::PageFault { address, code } = info.info() {
        if address == Virt::from_ptr(NonNull::<u8>::dangling().as_ptr()) {
            log(
                Level::ERROR,
                "an assertion has failed, hang up the test ##################################",
                address.into_usize(),
            );
            loop {
                hint::spin_loop();
            }
        }

        log(
            Level::INFO,
            "trap handler called for a page fault on an address",
            address.into_usize(),
        );

        my_assert!(!code.contains(PageFaultInfo::WRITE), "non-write page fault");

        let page_index = address.into_usize() / Page::SIZE;
        let block = map_pages(page_index, 1);
        fill_block(block, TRAP_HANDLER_VALUE);
    } else {
        log(Level::ERROR, "unexpected trap number", info.number());
        my_assert!(false);
    }
}

fn map_pages(
    index: usize,
    count: usize,
) -> Block<Page> {
    let flags = USER_RW;
    let block = Block::from_index(index, index + count);
    my_assert!(block.is_ok());
    let block = syscall::map(Pid::Current, block.unwrap(), flags);
    my_assert!(block.is_ok());
    block.unwrap()
}

fn fill_block(
    block: Block<Page>,
    value: usize,
) -> &'static mut [usize] {
    let slice = unsafe { block.try_into_mut_slice() };
    my_assert!(slice.is_ok());
    let slice = slice.unwrap();
    slice.fill(value);

    slice
}

fn log(
    level: Level,
    message: &str,
    value: usize,
) {
    my_assert!(syscall::log_value(level, message, value).is_ok());
}

fn generate_page_fault() -> ! {
    unsafe {
        NonNull::<u8>::dangling().as_ptr().read_volatile();
    }

    unreachable!();
}

fn panic_handler(_: &PanicInfo) {
    generate_page_fault();
}

const TRAP_STACK_PAGES: usize = 4;

const MAX_RECURSION_LEVEL: usize = 8;

const MAIN_VALUE: usize = 333333333;
const TRAP_HANDLER_VALUE: usize = 777777777;
const TRASH_VALUE: usize = 555555555;
