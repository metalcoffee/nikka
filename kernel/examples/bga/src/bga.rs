use core::mem;

use bitflags::bitflags;
use embedded_graphics_core::{
    geometry::Size,
    pixelcolor::PixelColor,
};
use x86::io;

use ku::{
    error::{
        Error::Unimplemented,
        Result,
    },
    memory::Phys,
};

use kernel::log::{
    error,
    info,
};

use crate::frame_buffer::FrameBuffer;

/// Инициализирует графический режим с разрешением `resolution` и
/// глубиной цвета, задаваемой `Color`.
/// Возвращает соответствующий
/// [видеобуфер](https://en.wikipedia.org/wiki/Framebuffer).
pub fn init<Color: Default + PixelColor>(resolution: Size) -> Result<FrameBuffer<Color>> {
    let frame_buffer = bga_frame_buffer()?;

    let version = version()?;

    let bpp = if version >= VBE_DISPI_ID2 {
        u32::try_from(mem::size_of::<Color>()).expect("Color should be a few bytes long") * u8::BITS
    } else {
        if mem::size_of::<Color>() != 1 {
            return Err(Unimplemented);
        }
        0
    };

    if version >= VBE_DISPI_ID3 {
        check_mode(resolution, bpp)?;
    }

    set_mode(resolution, bpp)?;

    FrameBuffer::new(frame_buffer, resolution)
}

/// Сканирует PCI-шину в поисках
/// [Bochs Graphics Adaptor](https://wiki.osdev.org/Bochs_VBE_Extensions).
/// Возвращает адрес его
/// [видеобуфера](https://en.wikipedia.org/wiki/Framebuffer).
fn bga_frame_buffer() -> Result<Phys> {
    /// Идентификатор производителя PCI-устройства BGA.
    const BGA_PCI_VENDOR_ID: u16 = 0x1234;

    /// Идентификатор PCI-устройства BGA.
    const BGA_PCI_DEVICE_ID: u16 = 0x1111;

    bitflags! {
        #[derive(Clone, Copy, Eq, PartialEq)]
        struct Bar: u32 {
            const ADDRESS_MASK_16 = 0x_FFF0;
            const ADDRESS_MASK_32 = 0x_FFFF_FFF0;
            const ADDRESS_MASK_IO = 0x_FFFF_FFFC;
            const PREFETCHABLE = 0b_1 << 3;
            const TYPE = 0b_11 << 1;
            const TYPE_32_BITS = 0b_00 << 1;
            const TYPE_16_BITS = 0b_01 << 1;
            const TYPE_64_BITS = 0b_10 << 1;
            const SPACE = 0b_1 << 0;
            #[allow(clippy::identity_op)]
            const SPACE_MEMORY = 0b_0 << 0;
            const SPACE_IO = 0b_1 << 0;
        }
    }

    for pci_device in tinypci::brute_force_scan() {
        if pci_device.vendor_id == BGA_PCI_VENDOR_ID && pci_device.device_id == BGA_PCI_DEVICE_ID {
            let frame_buffer_bar = Bar::from_bits(pci_device.bars[0]).ok_or(Unimplemented)?;
            if frame_buffer_bar & Bar::SPACE != Bar::SPACE_MEMORY {
                return Err(Unimplemented);
            }

            let address_type = frame_buffer_bar & Bar::TYPE;
            let frame_buffer = Phys::new_u64(match address_type {
                Bar::TYPE_16_BITS => u64::from((frame_buffer_bar & Bar::ADDRESS_MASK_16).bits()),
                Bar::TYPE_32_BITS => u64::from((frame_buffer_bar & Bar::ADDRESS_MASK_32).bits()),
                Bar::TYPE_64_BITS =>
                    (u64::from(pci_device.bars[1]) << u32::BITS) |
                        u64::from((frame_buffer_bar & Bar::ADDRESS_MASK_32).bits()),
                _ => return Err(Unimplemented),
            })?;

            info!(class = ?pci_device.full_class, %frame_buffer, "found Bochs Graphics Adaptor");
            return Ok(frame_buffer);
        }
    }

    Err(Unimplemented)
}

/// Возвращает версию
/// [Bochs Graphics Adaptor](https://wiki.osdev.org/Bochs_VBE_Extensions).
fn version() -> Result<u16> {
    let bga_id = bga_in(Register::VBE_DISPI_INDEX_ID);

    if (VBE_DISPI_ID0 ..= VBE_DISPI_ID5).contains(&bga_id) {
        info!(
            version = bga_id - VBE_DISPI_ID0,
            "Bochs VBE extension is available",
        );
        Ok(bga_id)
    } else {
        error!("Bochs VBE extension is not available");
        Err(Unimplemented)
    }
}

/// Проверяет доступность графического режима с разрешением `resolution` и глубиной цвета `bpp`.
fn check_mode(
    resolution: Size,
    bpp: u32,
) -> Result<()> {
    bga_out(Register::VBE_DISPI_INDEX_ENABLE, VBE_DISPI_GETCAPS);

    let max_width = u32::from(bga_in(Register::VBE_DISPI_INDEX_XRES));
    let max_height = u32::from(bga_in(Register::VBE_DISPI_INDEX_YRES));
    let max_bpp = u32::from(bga_in(Register::VBE_DISPI_INDEX_BPP));

    info!(max_width, max_height, max_bpp, "the max available mode");

    if resolution.width <= max_width && resolution.height <= max_height && bpp <= max_bpp {
        Ok(())
    } else {
        error!(%resolution, bpp, "the mode is not available");
        Err(Unimplemented)
    }
}

/// Устанавливает графический режим с разрешением `resolution` и глубиной цвета `bpp`.
fn set_mode(
    resolution: Size,
    bpp: u32,
) -> Result<()> {
    let message = "enormous resolution or bpp";

    bga_out(Register::VBE_DISPI_INDEX_ENABLE, 0);

    bga_out(
        Register::VBE_DISPI_INDEX_XRES,
        resolution.width.try_into().expect(message),
    );
    bga_out(
        Register::VBE_DISPI_INDEX_YRES,
        resolution.height.try_into().expect(message),
    );

    bga_out(
        Register::VBE_DISPI_INDEX_BPP,
        bpp.try_into().expect(message),
    );

    let acknowledged_width = u32::from(bga_in(Register::VBE_DISPI_INDEX_XRES));
    let acknowledged_height = u32::from(bga_in(Register::VBE_DISPI_INDEX_YRES));
    let acknowledged_bpp = u32::from(bga_in(Register::VBE_DISPI_INDEX_BPP));

    if acknowledged_width != resolution.width ||
        acknowledged_height != resolution.height ||
        acknowledged_bpp != bpp
    {
        error!(
            %resolution,
            bpp,
            acknowledged_width,
            acknowledged_height,
            acknowledged_bpp,
            "the mode was not acknowledged",
        );
        return Err(Unimplemented);
    }

    info!(%resolution, bpp, "setting the mode");

    bga_out(
        Register::VBE_DISPI_INDEX_ENABLE,
        VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
    );

    Ok(())
}

/// Записывает в регистр `register` устройства BGA значение `value`.
fn bga_out(
    register: Register,
    value: u16,
) {
    unsafe {
        io::outw(VBE_DISPI_IOPORT_INDEX, register as u16);
        io::outw(VBE_DISPI_IOPORT_DATA, value);
    }
}

/// Читает из регистра `register` устройства BGA.
fn bga_in(register: Register) -> u16 {
    unsafe {
        io::outw(VBE_DISPI_IOPORT_INDEX, register as u16);
        io::inw(VBE_DISPI_IOPORT_DATA)
    }
}

/// Индекс регистра устройства BGA.
#[allow(non_camel_case_types)]
#[repr(u16)]
enum Register {
    /// Version register index.
    VBE_DISPI_INDEX_ID = 0,

    /// X resolution register index.
    VBE_DISPI_INDEX_XRES = 1,

    /// Y resolution register index.
    VBE_DISPI_INDEX_YRES = 2,

    /// Bits per pixel register index.
    VBE_DISPI_INDEX_BPP = 3,

    /// Status register index.
    VBE_DISPI_INDEX_ENABLE = 4,
}

/// Index port number.
const VBE_DISPI_IOPORT_INDEX: u16 = 0x01CE;

/// Data port number.
const VBE_DISPI_IOPORT_DATA: u16 = 0x01CF;

/// VBE version 0.
const VBE_DISPI_ID0: u16 = 0xB0C0;

/// VBE version 2.
const VBE_DISPI_ID2: u16 = 0xB0C2;

/// VBE version 3.
const VBE_DISPI_ID3: u16 = 0xB0C3;

/// VBE version 5.
const VBE_DISPI_ID5: u16 = 0xB0C5;

/// Get BGA capabilities.
const VBE_DISPI_GETCAPS: u16 = 2;

/// Enable BGA.
const VBE_DISPI_ENABLED: u16 = 0x0001;

/// Use linear frame buffer.
const VBE_DISPI_LFB_ENABLED: u16 = 0x0040;
