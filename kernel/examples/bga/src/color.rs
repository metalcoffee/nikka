use derive_more::Display;
use embedded_graphics_core::pixelcolor::{
    PixelColor,
    RgbColor,
    raw::RawU16,
};

/// Тип для цвета пикселей в режиме 24 bits per pixel --- по 8 бит на канал.
pub type Rgb888 = embedded_graphics_core::pixelcolor::Rgb888;

/// Позволяет задавать цвета разной разрядности
/// через более привычные константы по 8 бит на канал.
pub trait From24Bpp {
    /// Переводит цвет в 24-битном пространстве (по 8 бит на канал)
    /// в близкий по восприятию цвет потенциально меньшей разрядности.
    fn from_24_bpp(color: u32) -> Self;
}

impl From24Bpp for Rgb888 {
    fn from_24_bpp(color: u32) -> Self {
        let (r, g, b) = split_24_bpp(color);
        Self::new(r, g, b)
    }
}

/// Тип для цвета пикселей в режиме 16 bits per pixel ---
/// 5 бит на красный канал, 6 бит на зелёный и 5 бит на синий.
#[derive(Clone, Copy, Debug, Default, Display, Eq, PartialEq)]
#[display(
    "{:02X}:{:02X}:{:02X}",
    (u32::from(self.r()) << COMPONENT_SHIFT_FOR_24_BPP) / (u32::from(Self::MAX_R) + 1),
    (u32::from(self.g()) << COMPONENT_SHIFT_FOR_24_BPP) / (u32::from(Self::MAX_G) + 1),
    (u32::from(self.b()) << COMPONENT_SHIFT_FOR_24_BPP) / (u32::from(Self::MAX_B) + 1),
)]
#[repr(C)]
pub struct Rgb565(u16);

impl Rgb565 {
    /// Переводит цвет в 24-битном пространстве (по 8 бит на канал)
    /// в близкий по восприятию цвет в 16-битном пространстве [`Rgb565`].
    const fn from_24_bpp_components(
        r: u8,
        g: u8,
        b: u8,
    ) -> Self {
        Self(
            ((r as u16 * (Self::MAX_R as u16 + 1)) >> COMPONENT_SHIFT_FOR_24_BPP) << Self::R_SHIFT |
                ((g as u16 * (Self::MAX_G as u16 + 1)) >> COMPONENT_SHIFT_FOR_24_BPP) <<
                    Self::G_SHIFT |
                ((b as u16 * (Self::MAX_B as u16 + 1)) >> COMPONENT_SHIFT_FOR_24_BPP) <<
                    Self::B_SHIFT,
        )
    }

    /// Переводит цвет в 24-битном пространстве (по 8 бит на канал)
    /// в близкий по восприятию цвет в 16-битном пространстве [`Rgb565`].
    const fn from_24_bpp(color: u32) -> Self {
        let (r, g, b) = split_24_bpp(color);
        Self::from_24_bpp_components(r, g, b)
    }

    /// Номер младшего бита красной компоненты.
    const R_SHIFT: u32 = 11;

    /// Номер младшего бита зелёной компоненты.
    const G_SHIFT: u32 = 5;

    /// Номер младшего бита синей компоненты.
    const B_SHIFT: u32 = 0;
}

impl From24Bpp for Rgb565 {
    fn from_24_bpp(color: u32) -> Self {
        Self::from_24_bpp(color)
    }
}

impl RgbColor for Rgb565 {
    fn r(&self) -> u8 {
        ((self.0 & Self::RED.0) >> Self::R_SHIFT) as u8
    }

    fn g(&self) -> u8 {
        ((self.0 & Self::GREEN.0) >> Self::G_SHIFT) as u8
    }

    fn b(&self) -> u8 {
        ((self.0 & Self::BLUE.0) >> Self::B_SHIFT) as u8
    }

    const MAX_R: u8 = (!0u16 >> Self::R_SHIFT) as u8;
    const MAX_G: u8 = (((1u16 << Self::R_SHIFT) - 1) >> Self::G_SHIFT) as u8;
    const MAX_B: u8 = (((1u16 << Self::G_SHIFT) - 1) >> Self::B_SHIFT) as u8;

    const BLACK: Self = Self::from_24_bpp(0x000000);
    const RED: Self = Self::from_24_bpp(0xFF0000);
    const GREEN: Self = Self::from_24_bpp(0x00FF00);
    const BLUE: Self = Self::from_24_bpp(0x0000FF);
    const YELLOW: Self = Self::from_24_bpp(0xFFFF00);
    const MAGENTA: Self = Self::from_24_bpp(0xFF00FF);
    const CYAN: Self = Self::from_24_bpp(0x00FFFF);
    const WHITE: Self = Self::from_24_bpp(0xFFFFFF);
}

impl PixelColor for Rgb565 {
    type Raw = RawU16;
}

/// Смешивает цвета `this` с весом `alphа / 256` с `other` с весом `1 - alpha / 256`.
pub fn mix<Color: From24Bpp + RgbColor>(
    this: Color,
    other: Color,
    alpha: u8,
) -> Color {
    /// The amount of bit shift for fixed-point arithmetic.
    const SHIFT: u32 = u8::BITS;

    // This is the same as `a = if alpha < 128 { a } else { a + 1 }`
    // mapping [0x00; 0xFF] -> [0x00; 0x100] \ { 0x80 }.
    // The mapping allows to divide by 0x100 instead of 0xFF later.
    // (That is just to shift right by 8 bits instead of real divide.)
    let a = u32::from(alpha) + (u32::from(alpha) >> (SHIFT - 1));

    let na = (1 << SHIFT) - a;

    let mix_component = |this_component, other_component, max_component| {
        let scale_component =
            |component| (u32::from(component) << SHIFT) / (u32::from(max_component) + 1);

        (scale_component(this_component) * a + scale_component(other_component) * na) >> SHIFT
    };

    Color::from_24_bpp(
        mix_component(this.r(), other.r(), Color::MAX_R) << R_SHIFT_FOR_24_BPP |
            mix_component(this.g(), other.g(), Color::MAX_G) << G_SHIFT_FOR_24_BPP |
            mix_component(this.b(), other.b(), Color::MAX_B) << B_SHIFT_FOR_24_BPP,
    )
}

/// Разбивает цвет в 24-битном пространстве на три канала по 8 бит в каждом.
const fn split_24_bpp(color: u32) -> (u8, u8, u8) {
    (
        (color >> R_SHIFT_FOR_24_BPP) as u8,
        (color >> G_SHIFT_FOR_24_BPP) as u8,
        (color >> B_SHIFT_FOR_24_BPP) as u8,
    )
}

/// Количество бит на канал в 24-битном пространстве цветов.
const COMPONENT_SHIFT_FOR_24_BPP: u32 = 8;

/// Номер младшего бита красной компоненты в 24-битном пространстве цветов.
const R_SHIFT_FOR_24_BPP: u32 = 2 * COMPONENT_SHIFT_FOR_24_BPP;

/// Номер младшего бита зелёной компоненты в 24-битном пространстве цветов.
#[allow(clippy::identity_op)]
const G_SHIFT_FOR_24_BPP: u32 = 1 * COMPONENT_SHIFT_FOR_24_BPP;

/// Номер младшего бита синей компоненты в 24-битном пространстве цветов.
#[allow(clippy::erasing_op)]
const B_SHIFT_FOR_24_BPP: u32 = 0 * COMPONENT_SHIFT_FOR_24_BPP;
