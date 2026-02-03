use ku::memory::size::MiB;

use kernel::{
    fs::{
        Kind,
        test_scaffolding::{
            BLOCK_SIZE,
            Bitmap,
            Inode,
            Superblock,
            block_cache_init,
        },
    },
    log::debug,
};

pub(super) fn simple_fs_superblock() -> Superblock {
    Superblock::format(BLOCK_COUNT, INODE_COUNT).unwrap()
}

pub(super) fn simple_fs(kind: Kind) -> (Bitmap, Bitmap, [Inode; INODE_COUNT]) {
    debug!(block_count = BLOCK_COUNT);

    block_cache_init(FS_DISK, BLOCK_COUNT, CACHE_BLOCK_COUNT).unwrap();

    let superblock = simple_fs_superblock();

    Bitmap::format(superblock.block_bitmap().start, superblock.blocks()).unwrap();
    Bitmap::format(superblock.inode_bitmap().start, 0 .. INODE_COUNT).unwrap();

    let block_bitmap = Bitmap::new(superblock.block_bitmap().start, superblock.blocks()).unwrap();
    let mut inode_bitmap = Bitmap::new(superblock.inode_bitmap().start, 0 .. INODE_COUNT).unwrap();
    let mut inodes = [Inode::default(); INODE_COUNT];
    assert_eq!(inode_bitmap.allocate(), Ok(0));
    inodes[0] = Inode::new(kind);

    (block_bitmap, inode_bitmap, inodes)
}

pub(super) const BLOCK_COUNT: usize = FS_SIZE / BLOCK_SIZE;
pub(super) const FS_DISK: usize = 1;
pub(super) const INODE_COUNT: usize = BLOCK_COUNT / 4;

const CACHE_BLOCK_COUNT: usize = BLOCK_COUNT / 4;
const FS_SIZE: usize = 32 * MiB;
