#![deny(warnings)]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use core::cmp;

use chrono::{
    DateTime,
    Duration,
    Utc,
};
use x86_64::instructions;

use ku::{
    self,
    time::{
        self,
        Hz,
        pit8254::TICKS_PER_SECOND,
        test_scaffolding::{
            NSECS_PER_SEC,
            datetime_with_resolution,
            forge_tsc,
            new_correlation_interval,
        },
    },
};

use kernel::{
    Subsystems,
    log::{
        debug,
        error,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

mod init;

init!(Subsystems::empty());

#[test_case]
fn points_beyond_interval() {
    wait_for_two_correlation_points();

    let a_few_seconds_in_tsc = 10_000_000_000;

    let before_boot = time::datetime(forge_tsc(-a_few_seconds_in_tsc));
    let boot = time::datetime(forge_tsc(0));
    let now = time::now();
    let future = time::datetime(forge_tsc(time::tsc() + a_few_seconds_in_tsc));

    debug!(%before_boot);
    debug!(%boot);
    debug!(%now);
    debug!(%future);

    check_difference(before_boot, boot);
    check_difference(boot, now);
    check_difference(now, future);

    fn check_difference(
        left: DateTime<Utc>,
        right: DateTime<Utc>,
    ) {
        let difference = right - left;

        debug!(%left, %right, %difference);

        assert!(left < right);
        assert!(difference > Duration::milliseconds(100));
        assert!(difference < Duration::seconds(30));
    }
}

#[test_case]
fn stress() {
    wait_for_two_correlation_points();

    stress::<1, 1_000_000>(2_000, 4_000, 3, 7);
    stress::<1, 1_000_000>(2_000, 100, 1, 1);

    fn stress<const TICKS_PER_SECOND: i64, const PARTS_PER_SECOND: i64>(
        tsc_per_tick: i64,
        tsc_end: i64,
        base_tsc_step: usize,
        tsc_step: usize,
    ) {
        let mock_cpu_frequency = Hz::new(tsc_per_tick.try_into().unwrap()).unwrap();

        let mock_rdtsc_interval = Duration::nanoseconds(NSECS_PER_SEC / tsc_per_tick);
        let min_delta = mock_rdtsc_interval / 2;

        let max_valid_delta = Duration::nanoseconds(tsc_end * NSECS_PER_SEC / tsc_per_tick);
        let max_delta = max_valid_delta * 2;

        debug!(
            %mock_cpu_frequency,
            %mock_rdtsc_interval,
            %min_delta,
            %max_valid_delta,
            %max_delta,
            "expected bounds for the real time delta between two points \
             for the given mock CPU frequency",
        );

        for base_tsc in (1 .. tsc_end).step_by(base_tsc_step) {
            let correlation_interval =
                new_correlation_interval::<TICKS_PER_SECOND>(base_tsc, base_tsc + tsc_per_tick);

            let tsc_0 = forge_tsc(0);
            let point_0 = datetime_with_resolution::<PARTS_PER_SECOND, TICKS_PER_SECOND>(
                &correlation_interval,
                tsc_0,
            );

            for tsc in (-tsc_end .. tsc_end).step_by(tsc_step) {
                if tsc == 0 {
                    continue;
                }

                let tsc_1 = forge_tsc(tsc);
                let point_1 = datetime_with_resolution::<PARTS_PER_SECOND, TICKS_PER_SECOND>(
                    &correlation_interval,
                    tsc_1,
                );

                let delta = if tsc < 0 {
                    point_0 - point_1
                } else {
                    point_1 - point_0
                };

                if delta < min_delta || max_delta < delta {
                    debug!(
                        ?tsc_0,
                        ?tsc_1,
                        "checking absolute real time delta between two mock RDTSC points",
                    );
                    debug!(
                        ?correlation_interval,
                        "correlation interval for converting RDTSC points into the real dates",
                    );
                    debug!(%point_0, %point_1, "points");

                    let message =
                        "delta between points converted into the real dates is out of bounds";
                    error!(%point_0, %point_1, %delta, %min_delta, %max_delta, message);
                    panic!("{}", message);
                }
            }
        }
    }
}

#[test_case]
fn high_resolution_date() {
    wait_for_two_correlation_points();

    const PIT_TICKS: i64 = 102;
    const NANOSECONDS_PER_PIT_TICK: i64 = 1_000_000_000 / TICKS_PER_SECOND as i64;
    let mut prev = DateTime::default();
    let timer_ticks = TRAP_STATS[Trap::Pit].count();
    let tick_duration = Duration::nanoseconds(NANOSECONDS_PER_PIT_TICK);
    let max_delta = Duration::nanoseconds(NANOSECONDS_PER_PIT_TICK * 13 / 10);
    let min_delta = Duration::nanoseconds(NANOSECONDS_PER_PIT_TICK * 7 / 10);

    let mut samples = 0;
    let mut max = Duration::min_value();
    let mut min = Duration::max_value();
    let mut sum = Duration::zero();
    let mut sum2 = 0;

    debug!(timer_ticks = PIT_TICKS, "measuring");

    for ticks in 1 .. PIT_TICKS {
        while TRAP_STATS[Trap::Pit].count() < timer_ticks + ticks as usize {
            instructions::hlt();
        }

        let now = time::now();

        if ticks > 1 {
            let delta = now - prev;

            if delta < min_delta || max_delta < delta {
                debug!(%ticks, %delta, %prev, %now);
            }

            samples += 1;
            max = cmp::max(max, delta);
            min = cmp::min(min, delta);
            sum += delta;
            let error = (delta - tick_duration).num_nanoseconds().unwrap();
            sum2 += error * error;
        }

        prev = now;
    }

    let max_mean = Duration::nanoseconds(NANOSECONDS_PER_PIT_TICK * 101 / 100);
    let min_mean = Duration::nanoseconds(NANOSECONDS_PER_PIT_TICK * 99 / 100);
    let max_deviation = Duration::milliseconds(5);
    let min_deviation = Duration::nanoseconds(100);

    let mean = sum / samples;
    let deviation = Duration::nanoseconds(num_integer::sqrt(sum2 / i64::from(samples)));

    debug!(samples, %min, %max, %mean, %deviation, "measured");
    debug!(
        %min_delta,
        %max_delta,
        %min_mean,
        %max_mean,
        %min_deviation,
        %max_deviation,
        "restrictions",
    );

    assert!(
        min_delta <= min && max <= max_delta,
        "one of the measured durations between PIT ticks is not accurate",
    );
    assert!(
        min_mean <= mean && mean <= max_mean,
        "measured mean PIT frequency is not accurate",
    );
    assert!(
        min_deviation <= deviation && deviation <= max_deviation,
        "measured deviation of the PIT frequency is too big",
    );
}

fn wait_for_two_correlation_points() {
    debug!("waiting for the RTC to tick twice");

    let rtc_ticks = TRAP_STATS[Trap::Rtc].count();
    while TRAP_STATS[Trap::Rtc].count() < rtc_ticks + 2 {
        instructions::hlt();
    }
}
