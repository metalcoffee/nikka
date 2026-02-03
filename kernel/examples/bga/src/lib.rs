#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

//! Обеспечивает работу библиотеки `embedded_graphics` поверх драйвера
//! [Bochs Graphics Adaptor](https://wiki.osdev.org/Bochs_VBE_Extensions).

#![deny(warnings)]
#![feature(int_roundings)]
#![no_std]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(missing_docs)]

/// [Bochs Graphics Adaptor](https://wiki.osdev.org/Bochs_VBE_Extensions).
///
/// [Документация](http://cvs.savannah.nongnu.org/viewvc/*checkout*/vgabios/vgabios/vbe_display_api.txt?revision=1.14).
/// Имена констант `VBE_...` взяты из неё.
pub mod bga;

/// Определяет тип для цвета пикселей.
pub mod color;

/// Управляет содержимым экрана через его
/// [видеобуфер](https://en.wikipedia.org/wiki/Framebuffer).
/// Поддерживает
/// [двойную буферизацию](https://en.wikipedia.org/wiki/Multiple_buffering).
pub mod frame_buffer;
