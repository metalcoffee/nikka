#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use core::{
    panic::PanicInfo,
    ptr::NonNull,
};

use ku::{
    error::{
        Error::{
            InvalidArgument,
            Overflow,
            PermissionDenied,
        },
        Result,
    },
    log::{
        self,
        Level,
    },
    memory::{
        Block,
        Page,
        Virt,
        block::Memory,
        size,
    },
    process::{
        ResultCode,
        Syscall,
    },
    time,
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

    let now = time::now();
    let timestamp = now.timestamp().try_into().unwrap();

    my_assert!(
        syscall::log_value(
            Level::INFO,
            "user space can read the system time",
            timestamp,
        )
        .is_ok()
    );

    log_kernel_block::<Page>(
        0,
        0,
        "expected Err(PermissionDenied) for logging Block::<Page>::from_index(0, 0), but got",
    );
    log_kernel_block::<Page>(
        0,
        1,
        "expected Err(PermissionDenied) for logging Block::<Page>::from_index(0, 1), but got",
    );
    log_kernel_block::<Page>(
        1,
        1,
        "expected Err(PermissionDenied) for logging Block::<Page>::from_index(1, 1), but got",
    );
    log_kernel_block::<Page>(
        1,
        2,
        "expected Err(PermissionDenied) for logging Block::<Page>::from_index(1, 2), but got",
    );

    log_kernel_block::<Virt>(
        0,
        0,
        "expected Err(PermissionDenied) for logging Block::<Virt>::from_index(0, 0), but got",
    );
    log_kernel_block::<Virt>(
        0,
        1,
        "expected Err(PermissionDenied) for logging Block::<Virt>::from_index(0, 1), but got",
    );
    log_kernel_block::<Virt>(
        1,
        1,
        "expected Err(PermissionDenied) for logging Block::<Virt>::from_index(1, 1), but got",
    );
    log_kernel_block::<Virt>(
        1,
        2,
        "expected Err(PermissionDenied) for logging Block::<Virt>::from_index(1, 2), but got",
    );

    let invalid_utf8 = b"\xFF";
    let result = log_block(Block::from_slice(invalid_utf8), 0);
    my_assert!(result.is_err(), "expected Err(InvalidArgument), got Ok");
    my_assert!(
        result == Err(InvalidArgument),
        "expected Err(InvalidArgument), got another error",
        ResultCode::from(result).into(),
    );

    let result = log_something(0x1_0000, 0xFFFF_FFFF_0000_0000, 0);
    my_assert!(result.is_err(), "expected Err(InvalidArgument), got Ok");
    my_assert!(
        result == Err(InvalidArgument),
        "expected Err(InvalidArgument), got another error",
        ResultCode::from(result).into(),
    );

    let result = log_something(0xFFFF_FFFF_FFFF_0000, 0x10_0000, 0);
    my_assert!(
        result.is_err(),
        "expected Err(InvalidArgument) or Err(Overflow), got Ok",
    );
    my_assert!(
        result == Err(InvalidArgument) || result == Err(Overflow),
        "expected Err(InvalidArgument) or Err(Overflow), got another error",
        ResultCode::from(result).into(),
    );
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

fn log_block(
    block: Block<Virt>,
    value: usize,
) -> Result<()> {
    log_something(block.start_address().into_usize(), block.size(), value)
}

fn log_something(
    start: usize,
    size: usize,
    value: usize,
) -> Result<()> {
    let level = size::from(u32::from(log::level_into_symbol(&Level::INFO)));

    syscall::syscall(Syscall::LogValue, level, start, size, value, 0).map(|_| ())
}

fn log_kernel_block<T: Memory>(
    start: usize,
    end: usize,
    message: &str,
) where
    Block<Virt>: From<Block<T>>,
{
    let block = Block::<T>::from_index(start, end);
    my_assert!(block.is_ok(), "the Block::from_index() should be ok");

    let result = log_block(block.unwrap().into(), 0);
    my_assert!(
        result == Err(PermissionDenied),
        message,
        ResultCode::from(result).into(),
    );
}
