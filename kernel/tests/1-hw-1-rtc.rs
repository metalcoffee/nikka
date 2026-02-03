#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

use chrono::{
    DateTime,
    Duration,
};
use x86_64::instructions;

use ku::time;

use kernel::{
    Subsystems,
    log::debug,
    time::test_scaffolding::{
        RegisterB,
        parse_hour,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

mod init;

init!(Subsystems::empty());

#[test_case]
fn different_rtc_formats() {
    for hour_24 in 0 .. 24 {
        let pm = hour_24 / 12;
        let hour_12 = ((hour_24 + 11) % 12) + 1;

        debug!(hour_24, hour_12, pm);

        assert_eq!(
            hour_24,
            parse_hour(
                hour_24,
                RegisterB::USE_BINARY_FORMAT | RegisterB::USE_24_HOUR_FORMAT,
            ),
        );
        assert_eq!(
            hour_24,
            parse_hour(pack(hour_12, pm), RegisterB::USE_BINARY_FORMAT),
        );
        assert_eq!(
            hour_24,
            parse_hour(bcd(hour_24), RegisterB::USE_24_HOUR_FORMAT),
        );
        assert_eq!(
            hour_24,
            parse_hour(pack(bcd(hour_12), pm), RegisterB::empty()),
        );
    }

    fn bcd(x: u8) -> u8 {
        (x / 10) * 16 + (x % 10)
    }

    fn pack(
        hour: u8,
        pm: u8,
    ) -> u8 {
        hour | (pm << 7)
    }
}

#[test_case]
fn rtc_read_inconsistent() {
    debug!("waiting for the RTC to tick at least once");
    while TRAP_STATS[Trap::Rtc].count() == 0 {
        instructions::hlt();
    }

    let start = time::now();
    let rtc_ticks = TRAP_STATS[Trap::Rtc].count();
    const TICKS: i64 = 10;

    debug!(%start);
    assert!(
        start > DateTime::parse_from_rfc3339("2022-09-10T11:50:17+00:00").unwrap(),
        "the RTC date does not pass the sanity check",
    );

    for ticks in 1 .. TICKS {
        let mut rtc_count = TRAP_STATS[Trap::Rtc].count();

        while rtc_count < rtc_ticks + (ticks as usize) {
            instructions::hlt();

            let new_rtc_count = TRAP_STATS[Trap::Rtc].count();
            if new_rtc_count != rtc_count {
                debug!(rtc_count, new_rtc_count);
                assert_eq!(new_rtc_count, rtc_count + 1, "unexpected RTC tick");
            }
            rtc_count = new_rtc_count;
        }

        let now = time::now();
        let max_now = start + Duration::seconds(ticks + 1);
        let min_now = start + Duration::seconds(ticks - 1);

        debug!(%now);

        assert!(
            min_now <= now && now <= max_now,
            "the RTC date does not follow its ticks",
        );
    }
}
