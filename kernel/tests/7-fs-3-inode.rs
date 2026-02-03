#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::{
    format,
    vec,
    vec::Vec,
};
use core::{
    cmp,
    mem,
};

use chrono::Duration;

use ku::memory::size::{
    MiB,
    Size,
};

use kernel::{
    Subsystems,
    fs::{
        BlockCache,
        File,
        FileSystem,
        Kind,
        test_scaffolding::{
            BLOCK_SIZE,
            block_count,
            flush_block,
            make_file,
            remove_file,
        },
    },
    log::debug,
    memory::Virt,
    time::{
        self,
        TscDuration,
    },
};

mod fs_helpers;
mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn block_entry() {
    let (mut block_bitmap, _, mut inodes) = fs_helpers::simple_fs(Kind::File);
    let inode = &mut inodes[0];

    const ENTRY_SIZE: usize = mem::size_of::<usize>();
    const ENTRIES_PER_BLOCK: usize = BLOCK_SIZE / ENTRY_SIZE;

    let mut consecutive_entries = ENTRIES_PER_BLOCK - 1;
    let mut prev_entry = Virt::default();
    let check_entries = 3 * ENTRIES_PER_BLOCK.pow(2);

    for i in 0 .. check_entries {
        let entry = inode.block_entry(i, &mut block_bitmap).unwrap();

        let current_entry = Virt::new(entry as *const _ as usize).unwrap();
        let prev_entry_inc = (prev_entry + ENTRY_SIZE).unwrap();
        if prev_entry >= current_entry || i % 100_000 == 0 {
            debug!(i, %prev_entry, %current_entry);
        }
        assert!(prev_entry < current_entry);

        if consecutive_entries != 0 && prev_entry != Virt::default() {
            assert_eq!(current_entry, prev_entry_inc);
        }

        consecutive_entries = (consecutive_entries + 1) % ENTRIES_PER_BLOCK;
        prev_entry = current_entry;

        *entry = i;
    }

    debug!("block entry allocation is done, checking the entries");

    for i in 0 .. check_entries {
        let entry = inode.block_entry(i, &mut block_bitmap).unwrap();
        if *entry != i || i % 100_000 == 0 {
            debug!(i, actual = *entry, expected = i);
        }
        assert_eq!(*entry, i);
    }

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn read_write_speed() {
    FileSystem::format(FS_DISK).unwrap();
    let block_count = block_count(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, block_count, RESOLVE_CACHE_SIZE).unwrap();
    let file = make_file(&mut fs, Kind::File);

    let mut buffer = vec![0x77_u8; 10 * MiB];
    let size = Size::from_slice(&buffer);
    let timer = time::timer();
    fs.write(&file, 0, &buffer).unwrap();
    let elapsed = timer.elapsed();
    debug!(
        throughput_per_second = %throughput_per_second(size, elapsed),
        %size,
        %elapsed,
        "disk read speed",
    );

    buffer.fill(0);

    let timeout = TscDuration::try_from(Duration::seconds(2)).unwrap();

    let timer = time::timer();
    fs.read(&file, 0, &mut buffer).unwrap();
    let elapsed = timer.elapsed();
    debug!(
        throughput_per_second = %throughput_per_second(size, elapsed),
        %size,
        %elapsed,
        %timeout,
        "file system read speed",
    );
    assert!(elapsed < timeout);

    assert_eq!(buffer[123456], 0x77);
    buffer.fill(0x33);

    let timer = time::timer();
    fs.write(&file, 0, &buffer).unwrap();
    let elapsed = timer.elapsed();
    debug!(
        throughput_per_second = %throughput_per_second(size, elapsed),
        %size,
        %elapsed,
        %timeout,
        "file system write speed",
    );
    assert!(elapsed < timeout);

    let timer = time::timer();
    for block in 0 .. fs_helpers::BLOCK_COUNT {
        flush_block(block).unwrap();
    }
    let elapsed = timer.elapsed();
    debug!(
        throughput_per_second = %throughput_per_second(size, elapsed),
        %size,
        %elapsed,
        "disk write speed",
    );

    debug!(block_cache_stats = ?BlockCache::stats());

    fn throughput_per_second(
        size: Size,
        elapsed: TscDuration,
    ) -> Size {
        let elapsed: Duration = elapsed.try_into().unwrap();
        let throughput_per_second = u128::try_from(size.num_bytes()).unwrap() * 1_000_000 /
            u128::try_from(elapsed.num_microseconds().unwrap()).unwrap();
        Size::bytes(throughput_per_second.try_into().unwrap())
    }
}

#[test_case]
fn write_read() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let file = make_file(&mut fs, Kind::File);

    let data: Vec<_> = (0 .. MiB).map(|x| (x + x * x) as u8).collect();
    let mut offset = 0;

    while offset < data.len() {
        for size in (0 ..= 3 * BLOCK_SIZE).filter(|x| (x + 3) % BLOCK_SIZE < 6) {
            let real_size = cmp::min(offset + size, data.len()) - offset;
            assert_eq!(
                fs.write(&file, offset, &data[offset .. offset + real_size]),
                Ok(real_size),
            );
            offset += real_size;
        }
    }

    let mut buffer = [0; MiB];
    offset = 0;

    while offset < data.len() {
        for size in (0 ..= 4 * BLOCK_SIZE).filter(|x| (x + 4) % BLOCK_SIZE < 7) {
            let real_size = cmp::min(offset + size, data.len()) - offset;
            assert_eq!(
                fs.read(&file, offset, &mut buffer[offset .. offset + real_size]),
                Ok(real_size),
            );
            offset += real_size;
        }
    }

    debug!(actual = ?buffer[..10], expected = ?data[..10]);
    for (actual, expected) in buffer.iter().zip(data) {
        assert_eq!(*actual, expected);
    }

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn big_file() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let file = make_file(&mut fs, Kind::File);

    let start_free_block_count = fs.free_space() / BLOCK_SIZE;

    let file_block_count = start_free_block_count * 99 / 100;
    fs.set_size(&file, file_block_count * BLOCK_SIZE).unwrap();
    debug!(file_block_count, file_size = %Size::bytes(fs.size(&file)));

    for file_block in 0 .. file_block_count {
        let data = format!("file block {file_block}");
        let data_len = data.len();
        assert_eq!(
            fs.write(&file, file_block * BLOCK_SIZE, data.as_bytes()),
            Ok(data_len),
        );
    }

    for file_block in 0 .. file_block_count {
        let data = format!("file block {file_block}");
        let data_len = data.len();
        let mut buffer = vec![0; data_len];
        assert_eq!(
            fs.read(&file, file_block * BLOCK_SIZE, &mut buffer),
            Ok(data_len),
        );
        assert_eq!(buffer, data.as_bytes());
    }

    let free_block_count = fs.free_space() / BLOCK_SIZE;
    let used_block_count = start_free_block_count - free_block_count;
    debug!(free_block_count, used_block_count);

    remove_file(&mut fs, &file).unwrap();

    let end_free_block_count = fs.free_space() / BLOCK_SIZE;
    assert_eq!(start_free_block_count, end_free_block_count);

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn set_size() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let file = make_file(&mut fs, Kind::File);

    let start_free_block_count = fs.free_space() / BLOCK_SIZE;

    let filter = |x: &usize| (x + 2) % (BLOCK_SIZE / 3) <= 4;

    for offset in (0 ..= 2 * BLOCK_SIZE).filter(filter) {
        for size in (0 ..= 2 * BLOCK_SIZE).filter(filter) {
            if offset == size {
                debug!(offset, size);
            }
            check_expansion_is_zero_filled(&mut fs, &file, offset, size);
            fs.set_size(&file, offset).unwrap();
            check_expansion_is_zero_filled(&mut fs, &file, offset, size);
            fs.set_size(&file, offset).unwrap();
        }
    }

    fs.set_size(&file, 0).unwrap();
    let end_free_block_count = fs.free_space() / BLOCK_SIZE;
    assert_eq!(start_free_block_count, end_free_block_count);

    fn check_expansion_is_zero_filled(
        fs: &mut FileSystem,
        file: &File,
        offset: usize,
        size: usize,
    ) {
        let mut buffer = vec![1; size];

        fs.set_size(file, offset + size).unwrap();
        fs.read(file, offset, &mut buffer).unwrap();
        assert!(buffer.iter().all(|&x| x == 0));

        buffer.fill(b'*');
        assert_eq!(fs.write(file, offset, &buffer), Ok(size));
        fs.read(file, offset, &mut buffer).unwrap();
        assert!(buffer.iter().all(|&x| x == b'*'));
    }

    debug!(block_cache_stats = ?BlockCache::stats());
}

const CACHE_BLOCK_COUNT: usize = 1 << 10;
const FS_DISK: usize = 1;
const RESOLVE_CACHE_SIZE: usize = 5;
