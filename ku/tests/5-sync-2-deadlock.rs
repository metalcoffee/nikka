#![deny(warnings)]

use std::time::Duration;

use rstest::rstest;

use ku::{
    log::debug,
    sync::Spinlock,
};

mod log;

#[rstest]
#[should_panic(expected = "deadlock")]
#[timeout(Duration::from_secs(10))]
fn deadlock_in_panic_message() {
    recursive_deadlock()
}

#[rstest]
#[should_panic(expected = "ku/tests/5-sync-2-deadlock.rs:44")]
#[timeout(Duration::from_secs(10))]
fn definition_line_number_in_panic_message() {
    recursive_deadlock()
}

#[rstest]
#[should_panic(expected = "ku/tests/5-sync-2-deadlock.rs:45")]
#[timeout(Duration::from_secs(10))]
fn owner_line_number_in_panic_message() {
    recursive_deadlock()
}

#[rstest]
#[should_panic(expected = "ku/tests/5-sync-2-deadlock.rs:48")]
#[timeout(Duration::from_secs(10))]
fn deadlock_line_number_in_panic_message() {
    recursive_deadlock()
}

/// Строки, ссылки на которые должны попасть в сообщение паники, отмечены комментарием `// (*)`.
fn recursive_deadlock() {
    let spinlock = Spinlock::new(0); // (*)
    let _lock = spinlock.lock(); // (*)

    debug!(?spinlock, "attempting to lock a locked spinlock");
    drop(spinlock.lock()); // (*)
    debug!(error = "no panic occurred");
}

#[ctor::ctor]
fn init() {
    log::init();
}
