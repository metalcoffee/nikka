#![deny(warnings)]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::{
    cmp,
    sync::atomic::{
        AtomicI64,
        AtomicUsize,
        Ordering,
    },
};

use x86_64::instructions;

use ku::{
    memory::{
        Block,
        Virt,
    },
    process::RFlags,
    time::{
        self,
        test_scaffolding::AtomicCorrelationPoint,
    },
};

use kernel::{
    Subsystems,
    log::debug,
    trap::{
        self,
        TRAP_STATS,
        Trap,
        TrapContext,
    },
};

mod init;

init!(Subsystems::empty());

#[test_case]
fn correlation_point_reader() {
    trap::test_scaffolding::set_debug_handler(writer);

    reader();

    static POINT: AtomicCorrelationPoint = AtomicCorrelationPoint::new();

    fn reader() {
        let mut prev = POINT.load();
        let mut different = 0;
        let mut same = 0;
        let rtc_ticks = AtomicUsize::new(0);

        const ITERATIONS: usize = 10_000;

        while same < ITERATIONS || different < ITERATIONS {
            switch_trap_flag();
            let point = POINT.load();
            switch_trap_flag();

            if prev == point {
                same += 1;
            } else {
                different += 1;
            }

            if sample(&rtc_ticks) {
                debug!(same, different, ?point);
            }

            assert_eq!(2 * point.count(), point.tsc(), "{point:?} is inconsistent");

            prev = point;
        }
    }

    extern "x86-interrupt" fn writer(_: TrapContext) {
        static VALUE: AtomicI64 = AtomicI64::new(0);

        let mut value = VALUE.fetch_add(1, Ordering::Relaxed);
        if value % 37 == 17 {
            value -= 1;
        }

        match (value / 1_000) % 4 {
            0 => POINT.store(time::test_scaffolding::new_point(value + 1, 2 * value + 2)),
            2 => POINT.inc(2 * POINT.load().count() + 2),
            _ => {},
        }
    }
}

#[test_case]
fn correlation_point_writer() {
    trap::test_scaffolding::set_debug_handler(reader);

    writer();

    const LAST_ITERATION: i64 = 10_000;

    static ITERATION: AtomicI64 = AtomicI64::new(0);
    static POINT: AtomicCorrelationPoint = AtomicCorrelationPoint::new();

    fn writer() {
        switch_trap_flag();

        for iteration in 0 .. LAST_ITERATION / 2 {
            let value = iteration -
                if iteration % 37 == 17 {
                    1
                } else {
                    0
                };
            POINT.store(time::test_scaffolding::new_point(value, 2 * value));
            ITERATION.store(iteration, Ordering::Relaxed);
        }

        for iteration in LAST_ITERATION / 2 ..= LAST_ITERATION {
            POINT.inc(2 * POINT.load().count() + 2);
            ITERATION.store(iteration, Ordering::Relaxed);
        }

        switch_trap_flag();
    }

    extern "x86-interrupt" fn reader(_: TrapContext) {
        static FAILURE_COUNT: AtomicI64 = AtomicI64::new(0);
        static SUCCESS_COUNT: AtomicI64 = AtomicI64::new(0);
        static RTC_TICKS: AtomicUsize = AtomicUsize::new(0);

        let iteration = ITERATION.load(Ordering::Relaxed);
        let failure_count = FAILURE_COUNT.load(Ordering::Relaxed);
        let success_count = SUCCESS_COUNT.load(Ordering::Relaxed);

        if let Some(point) = time::test_scaffolding::try_load(&POINT) {
            if sample(&RTC_TICKS) {
                debug!(iteration, failure_count, success_count, ?point);
                ITERATION.fetch_add(1, Ordering::Relaxed);
            }
            assert_eq!(2 * point.count(), point.tsc(), "{point:?} is inconsistent");
            SUCCESS_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        if iteration == LAST_ITERATION {
            debug!(iteration, failure_count, success_count);
            assert!(
                success_count >= LAST_ITERATION && failure_count >= LAST_ITERATION,
                "the test is too weak",
            );
            ITERATION.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ANCHOR: down_the_rabbit_hole
#[allow(dead_code)]
// #[test_case] // Uncomment it if you wish.
fn down_the_rabbit_hole() {
    trap::test_scaffolding::set_debug_handler(collect_statistics);

    wait_for_two_correlation_points();

    let mut timer = ku::timer();
    debug!(rflags = %RFlags::read(), "how many instructions does it take to log something?");
    let time_to_log_a_message = timer.lap();

    switch_trap_flag();
    debug!(rflags = %RFlags::read(), "how many instructions does it take to log something?");
    switch_trap_flag();
    let time_to_log_a_message_in_the_stepping_mode = timer.elapsed();

    let max_rsp = MAX_RSP.load(Ordering::Relaxed);
    let min_rsp = cmp::min(max_rsp, MIN_RSP.load(Ordering::Relaxed));
    let rip = RIP.load(Ordering::Relaxed);

    debug!(
        instruction_count = INSTRUCTION_COUNT.load(Ordering::Relaxed),
        used_stack_space = ?Block::<Virt>::from_index(min_rsp, max_rsp).unwrap(),
        last_traced_instruction_address = %Virt::new(rip).unwrap(),
    );

    let stepping_slowdown_ratio =
        time_to_log_a_message_in_the_stepping_mode.into_f64() / time_to_log_a_message.into_f64();
    debug!(
        %time_to_log_a_message,
        %time_to_log_a_message_in_the_stepping_mode,
        stepping_slowdown_ratio,
    );

    static INSTRUCTION_COUNT: AtomicUsize = AtomicUsize::new(0);
    static MAX_RSP: AtomicUsize = AtomicUsize::new(usize::MIN);
    static MIN_RSP: AtomicUsize = AtomicUsize::new(usize::MAX);
    static RIP: AtomicUsize = AtomicUsize::new(0);

    extern "x86-interrupt" fn collect_statistics(context: TrapContext) {
        let context = context.get().mini_context();
        let rip = context.rip().into_usize();
        let rsp = context.rsp().into_usize();

        let max_rsp = cmp::max(rsp, MAX_RSP.load(Ordering::Relaxed));
        let min_rsp = cmp::min(rsp, MIN_RSP.load(Ordering::Relaxed));

        INSTRUCTION_COUNT.fetch_add(1, Ordering::Relaxed);
        MAX_RSP.store(max_rsp, Ordering::Relaxed);
        MIN_RSP.store(min_rsp, Ordering::Relaxed);
        RIP.store(rip, Ordering::Relaxed);
    }
}
// ANCHOR_END: down_the_rabbit_hole

fn sample(ticks: &AtomicUsize) -> bool {
    let prev_rtc_ticks = ticks.load(Ordering::Relaxed);
    let rtc_ticks = TRAP_STATS[Trap::Rtc].count();
    if prev_rtc_ticks != rtc_ticks {
        ticks.store(rtc_ticks, Ordering::Relaxed);
        true
    } else {
        false
    }
}

fn switch_trap_flag() {
    let new_flags = RFlags::read() ^ RFlags::TRAP_FLAG;
    unsafe {
        new_flags.write();
    }
}

fn wait_for_two_correlation_points() {
    debug!("waiting for the RTC to tick twice");

    let rtc_ticks = TRAP_STATS[Trap::Rtc].count();
    while TRAP_STATS[Trap::Rtc].count() < rtc_ticks + 2 {
        instructions::hlt();
    }
}
