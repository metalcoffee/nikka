/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// фиксированного размера
/// для отслеживания какие именно элементы заняты, а какие --- свободны.
mod bitmap;

/// [Битовая карта](https://en.wikipedia.org/wiki/Free-space_bitmap)
/// расширяемого размера
/// для отслеживания какие именно элементы заняты, а какие --- свободны.
mod dynamic_bitmap;

/// LRU--кэш
/// ([Least Recently Used](https://en.wikipedia.org/wiki/Cache_replacement_policies#LRU)) ---
/// кэш с реализацией алгоритма вытеснения давно неиспользуемых данных.
mod lru;

pub use bitmap::Bitmap;
pub use dynamic_bitmap::DynamicBitmap;
pub use lru::Lru;
