mod big;
mod big_for_pipe;
mod cache;
mod caching_big;
mod fallback;

#[allow(unused)]
pub use big::Big;

#[allow(unused)]
pub use big_for_pipe::BigForPipe;

#[allow(unused)]
pub use cache::ThreadLocalCache;

#[allow(unused)]
pub use caching_big::CachingBig;

#[allow(unused)]
pub use fallback::Fallback;

use std::ffi::c_void;

use ku::memory::{
    Block,
    Page,
};

fn block(
    ptr: *mut c_void,
    len: usize,
) -> Block<Page> {
    unsafe {
        Block::from_index_count_unchecked((ptr as usize) / Page::SIZE, len.div_ceil(Page::SIZE))
    }
}
