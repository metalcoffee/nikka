#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::{
    cmp,
    fmt::Write,
    panic::PanicInfo,
};

use bootloader::{
    BootInfo,
    entry_point,
};
use chrono::Duration;
use embedded_graphics::{
    Drawable,
    geometry::{
        Dimensions,
        Point,
        Size,
    },
    mono_font::{
        MonoTextStyle,
        ascii::FONT_10X20,
    },
    primitives::{
        Primitive,
        PrimitiveStyleBuilder,
        Rectangle,
    },
    text::Text,
};
use embedded_plots::{
    axis::Scale,
    curve::{
        Curve,
        PlotPoint,
    },
    single_plot::SinglePlot,
};

use ku::{
    backtrace::Backtrace,
    error::Result,
};
use text::{
    Attribute,
    println,
};

use kernel::{
    self,
    Subsystems,
    log::{
        debug,
        info,
    },
    time::{
        self,
        rtc,
    },
    trap::{
        TRAP_STATS,
        Trap,
    },
};

use ::bga::{
    bga,
    color::{
        self,
        From24Bpp,
        Rgb565,
    },
    frame_buffer::FrameBuffer,
};

entry_point!(kernel_main);

type Color = Rgb565;

const SCREEN_SIZE: Size = Size::new(1024, 768);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    kernel::init_subsystems(boot_info, Subsystems::MEMORY);

    let mut frame_buffer = bga::init(SCREEN_SIZE).unwrap();

    let foreground = Color::from_24_bpp(0xC0C0FF);
    let background = Color::from_24_bpp(0x000040);

    let plot_x_span = 300;

    let mut prev_timer_count = TRAP_STATS[Trap::Rtc].count();

    let mut rtc_error = Vec::new();

    let plot_frame = frame_buffer.bounding_box();

    make_frame(&mut frame_buffer, &plot_frame, foreground, background).unwrap();

    frame_buffer.flush();

    loop {
        let mut flush = false;

        if prev_timer_count < TRAP_STATS[Trap::Rtc].count() {
            let timer = time::timer();
            make_frame(&mut frame_buffer, &plot_frame, foreground, background).unwrap();
            debug!(duration = %timer.elapsed(), "plot frame");

            let error = {
                let rtc_error = rtc::error();
                info!(%rtc_error);
                rtc_error.num_microseconds().and_then(|x| x.try_into().ok()).unwrap_or(
                    if rtc_error < Duration::zero() {
                        i32::MIN
                    } else {
                        i32::MAX
                    },
                )
            };

            if error.abs() < 100_000 {
                rtc_error.push(PlotPoint {
                    x: TRAP_STATS[Trap::Rtc].count() as i32,
                    y: error,
                });
            }

            let from = if rtc_error.len() > plot_x_span {
                rtc_error.len() - plot_x_span
            } else {
                0
            };

            let timer = time::timer();
            make_chart(
                &mut frame_buffer,
                &plot_frame,
                foreground,
                &rtc_error[from ..],
            )
            .unwrap();
            debug!(duration = %timer.elapsed(), "plot chart");

            flush = true;

            if rtc_error.len() > 3 * plot_x_span {
                rtc_error.drain(.. plot_x_span);
            }
        }

        prev_timer_count = TRAP_STATS[Trap::Rtc].count();

        if flush {
            frame_buffer.flush();
        }

        x86_64::instructions::hlt();
    }
}

fn make_frame(
    frame_buffer: &mut FrameBuffer<Color>,
    plot_frame: &Rectangle,
    foreground: Color,
    background: Color,
) -> Result<()> {
    let character_width = 10i32;
    let character_height = 20i32;
    let text_style = MonoTextStyle::new(&FONT_10X20, foreground);

    let frame_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(foreground)
        .fill_color(color::mix(background, foreground, 0xE0))
        .build();

    plot_frame.into_styled(frame_style).draw(frame_buffer)?;

    let text_point = plot_frame.top_left + Point::new(character_width, character_height);

    Text::new("RTC error in microseconds", text_point, text_style).draw(frame_buffer)?;

    Ok(())
}

fn make_chart(
    frame_buffer: &mut FrameBuffer<Color>,
    plot_frame: &Rectangle,
    foreground: Color,
    data: &[PlotPoint],
) -> Result<()> {
    if data.len() < 2 {
        return Ok(());
    }

    let plot_frame_border = Point::new(50, 50);
    let curve = Curve::from_data(data);

    let plot = SinglePlot::new(
        &curve,
        Scale::RangeFraction(cmp::min(15, data.len() - 1)),
        Scale::RangeFraction(10),
    )
    .into_drawable(
        plot_frame.top_left + plot_frame_border,
        plot_frame.bottom_right().unwrap() - plot_frame_border,
    )
    .set_thickness(1)
    .set_axis_thickness(1)
    .set_color(foreground)
    .set_text_color(foreground);

    plot.draw(frame_buffer)
}

#[cold]
#[inline(never)]
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    text::TEXT
        .lock()
        .set_attribute(Attribute::new(text::Color::WHITE, text::Color::RED));

    println!("{panic_info}");

    if let Ok(backtrace) = Backtrace::current() {
        println!("{backtrace:?}");
    }

    unsafe { ku::halt() }
}
