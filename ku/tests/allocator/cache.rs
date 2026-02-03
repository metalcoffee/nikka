use std::cell::RefCell;

use ku::allocator::{
    Cache,
    Clip,
    FIXED_SIZE_COUNT,
};

pub struct ThreadLocalCache;

impl ThreadLocalCache {
    #[allow(unused)]
    pub const fn new() -> Self {
        Self
    }
}

impl Cache for ThreadLocalCache {
    fn with_borrow_mut<F: FnOnce(&mut Clip) -> R, R>(
        &self,
        index: usize,
        f: F,
    ) -> R {
        CACHE.with_borrow_mut(|cache| f(&mut cache[index]))
    }

    const CACHE_AVAILABLE: bool = true;
}

thread_local! {
    static CACHE: RefCell<[Clip; FIXED_SIZE_COUNT]> =
        const { RefCell::new([const { Clip::new() }; FIXED_SIZE_COUNT]) };
}
