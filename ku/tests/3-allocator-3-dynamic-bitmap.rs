#![deny(warnings)]
#![feature(allocator_api)]

use rstest::rstest;

use ku::{
    collections::DynamicBitmap,
    log::debug,
    memory::{
        Page,
        size,
    },
};

use allocator::Fallback;

mod allocator;
mod log;

#[rstest]
fn basic() {
    let allocator = Fallback::new();
    let mut bitmap = DynamicBitmap::new();
    let mut bitmap_frame_count = 0;

    let elements_per_page = Page::SIZE * size::from(u8::BITS);
    let max_page_count = 4;
    let max_len = max_page_count * elements_per_page;

    bitmap.reserve(max_len, 1, &allocator).unwrap();

    let is_free_on_map = |bit| bit % 37 != 0;
    let mut free_bits = 0;
    let gen_len = (0 ..= max_len).filter(|x| (x + 5) % elements_per_page < 10);

    for (prev_len, len) in gen_len.clone().zip(gen_len.skip(1)) {
        bitmap_frame_count += bitmap.map(len, &allocator).unwrap();
        free_bits += len - prev_len;
        debug!(len, free_bits);
        assert_eq!(bitmap.free(), free_bits);
        assert_eq!(bitmap_frame_count, len.div_ceil(elements_per_page));

        for bit in 0 .. len {
            let should_be_free = bit >= prev_len || is_free_on_map(bit);

            assert_eq!(bitmap.is_free(bit), should_be_free);

            if !should_be_free {
                bitmap.set_free(bit);
                free_bits += 1;
            }

            assert_eq!(bitmap.free(), free_bits);
        }

        assert_eq!(bitmap.free(), len);
        assert_eq!(bitmap.len(), len);
        assert_eq!(bitmap.is_empty(), len == 0);

        for free in (0 .. len).rev() {
            assert_eq!(bitmap.free(), free + 1);

            bitmap.allocate().expect("failed to allocate a supposedly free element");

            assert_eq!(bitmap.free(), free);
        }

        assert_eq!(bitmap.free(), 0);
        assert_eq!(
            bitmap.allocate(),
            None,
            "allocated from a bitmap without free elements",
        );

        for bit in 0 .. len {
            assert!(!bitmap.is_free(bit));
        }

        free_bits = 0;

        for bit in 0 .. len {
            if is_free_on_map(bit) || len == max_len {
                bitmap.set_free(bit);
                free_bits += 1;
            }

            assert_eq!(bitmap.free(), free_bits);
        }
    }

    assert_eq!(bitmap.unmap(&allocator).unwrap(), bitmap_frame_count);
    assert!(bitmap.is_empty());
}

#[ctor::ctor]
fn init() {
    log::init();
}
