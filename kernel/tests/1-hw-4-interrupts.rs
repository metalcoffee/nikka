#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use ku::{
    log::debug,
    process::RFlags,
    sync::{
        IrqSpinlock,
        PanicStrategy,
    },
};

use kernel::Subsystems;

mod init;

init!(Subsystems::empty());

#[test_case]
fn interrupts_enabled() {
    let spinlock = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);

    check_interrupts("start", true);

    let mut lock = spinlock.lock();
    check_interrupts("locked", false);

    *lock += 1;

    drop(lock);
    check_interrupts("unlocked", true);

    let mut lock = spinlock.try_lock().unwrap();
    check_interrupts("try_lock() locked", false);
    check_interrupts("try_lock() succeeded", false);

    *lock += 1;

    drop(lock);
    check_interrupts("unlocked", true);
}

#[test_case]
fn interrupts_disabled() {
    let spinlock = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);

    disable_interrupts();

    check_interrupts("start", false);

    let mut lock = spinlock.lock();
    check_interrupts("locked", false);

    *lock += 1;

    drop(lock);
    check_interrupts("unlocked", false);

    let mut lock = spinlock.try_lock().unwrap();
    check_interrupts("try_lock() succeeded", false);

    *lock += 1;

    drop(lock);
    check_interrupts("unlocked", false);

    enable_interrupts();
}

#[test_case]
fn failed_try_lock() {
    let spinlock = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);

    check_interrupts("start", true);

    let mut lock = spinlock.lock();
    check_interrupts("locked", false);

    enable_interrupts();
    check_interrupts("enabled interrupts", true);

    *lock += 1;

    assert!(spinlock.try_lock().is_none());
    check_interrupts("try_lock() failed", true);

    drop(lock);
    check_interrupts("unlocked", true);
}

#[test_case]
fn nested() {
    let outer = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);
    let inner = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);

    check_interrupts("start", true);

    let mut outer_lock = outer.lock();
    check_interrupts("locked outer", false);

    let inner_lock = inner.lock();
    check_interrupts("locked inner", false);

    *outer_lock += *inner_lock;

    drop(inner_lock);
    check_interrupts("unlocked inner", false);

    drop(outer_lock);
    check_interrupts("unlocked outer", true);
}

#[allow(unused)] // TODO: requires CPU-local storage
fn non_lifo_drop_order() {
    let outer = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);
    let inner = IrqSpinlock::<_, { PanicStrategy::KnockDown }>::new(0);

    check_interrupts("start", true);

    let mut outer_lock = outer.lock();
    check_interrupts("locked outer", false);

    let inner_lock = inner.lock();
    check_interrupts("locked inner", false);

    *outer_lock += *inner_lock;

    drop(outer_lock);
    check_interrupts("unlocked outer", false);

    drop(inner_lock);
    check_interrupts("unlocked inner", true);
}

fn check_interrupts(
    message: &str,
    interrupts_should_be_enabled: bool,
) {
    let interrupts_enabled = RFlags::read().contains(RFlags::INTERRUPT_FLAG);
    debug!(interrupts_enabled, "{}", message);
    assert_eq!(interrupts_enabled, interrupts_should_be_enabled);
}

fn disable_interrupts() {
    let interrupts_enabled = RFlags::read().contains(RFlags::INTERRUPT_FLAG);
    assert!(interrupts_enabled);
    unsafe {
        (RFlags::read() & !RFlags::INTERRUPT_FLAG).write();
    }
}

fn enable_interrupts() {
    unsafe {
        (RFlags::read() | RFlags::INTERRUPT_FLAG).write();
    }
    let interrupts_enabled = RFlags::read().contains(RFlags::INTERRUPT_FLAG);
    assert!(interrupts_enabled);
}
