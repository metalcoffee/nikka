#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::format;
use core::str;

use ku::{
    error::Error::{
        FileExists,
        FileNotFound,
        InvalidArgument,
        NotDirectory,
        NotFile,
    },
    memory::size::Size,
};

use kernel::{
    Subsystems,
    fs::{
        BlockCache,
        FileSystem,
        Kind,
        MAX_NAME_LEN,
        test_scaffolding::{
            BLOCK_SIZE,
            make_file,
            remove_file,
        },
    },
    log::debug,
};

mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn basic_operations() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let directory = make_file(&mut fs, Kind::Directory);

    let mut buffer = [0; 1];
    assert_eq!(fs.read(&directory, 0, &mut buffer).unwrap_err(), NotFile);
    assert_eq!(fs.write(&directory, 0, &buffer).unwrap_err(), NotFile);

    assert!(fs.list(&directory).unwrap().is_empty());

    assert_eq!(fs.find(&directory, "file-1").unwrap_err(), FileNotFound);
    let file_1 = fs.insert(&directory, "file-1", Kind::File).unwrap();
    assert!(fs.find(&directory, "file-1").is_ok());

    assert_eq!(fs.list(&file_1).unwrap_err(), NotDirectory);
    assert_eq!(
        fs.insert(&file_1, "file-2", Kind::File).unwrap_err(),
        NotDirectory,
    );
    assert_eq!(fs.find(&file_1, "file-1").unwrap_err(), NotDirectory);

    let list = fs.list(&directory).unwrap();
    debug!(?list);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind(), Kind::File);
    assert_eq!(list[0].name(), "file-1");
    assert_eq!(list[0].size(), 0);

    assert_eq!(fs.write(&file_1, 0, &buffer), Ok(buffer.len()));

    let list = fs.list(&directory).unwrap();
    debug!(?list);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind(), Kind::File);
    assert_eq!(list[0].name(), "file-1");
    assert_eq!(list[0].size(), buffer.len());

    for kind in [Kind::Directory, Kind::File] {
        assert_eq!(
            fs.insert(&directory, "file-1", kind).unwrap_err(),
            FileExists,
        );
    }

    let offset_2 = 1234;
    let file_2 = fs.insert(&directory, "file-2", Kind::File).unwrap();
    assert_eq!(fs.write(&file_2, offset_2, &buffer), Ok(buffer.len()));

    let mut list = fs.list(&directory).unwrap();
    introsort::sort_by(&mut list, &|a, b| a.name().cmp(b.name()));
    debug!(?list);

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].kind(), Kind::File);
    assert_eq!(list[0].name(), "file-1");
    assert_eq!(list[0].size(), buffer.len());
    assert_eq!(list[1].kind(), Kind::File);
    assert_eq!(list[1].name(), "file-2");
    assert_eq!(list[1].size(), offset_2 + buffer.len());

    fs.remove(&file_1).unwrap();
    assert_eq!(fs.find(&directory, "file-1").unwrap_err(), FileNotFound);
    fs.remove(&file_2).unwrap();
    assert_eq!(fs.find(&directory, "file-2").unwrap_err(), FileNotFound);

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn big_directory() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let directory = make_file(&mut fs, Kind::Directory);

    let start_free_block_count = fs.free_space() / BLOCK_SIZE;

    for file_count in 1 ..= 1000 {
        fs.insert(&directory, &format!("file-{file_count}"), Kind::File).unwrap();
        if file_count % 100 == 0 {
            debug!(file_count, directory_size = %Size::bytes(fs.size(&directory)));
        }
    }

    let free_block_count = fs.free_space() / BLOCK_SIZE;
    let used_block_count = start_free_block_count - free_block_count;
    debug!(free_block_count, used_block_count);

    remove_file(&mut fs, &directory).unwrap();

    let end_free_block_count = fs.free_space() / BLOCK_SIZE;
    let leaked_block_count = start_free_block_count - end_free_block_count;
    assert_eq!(leaked_block_count, 0);

    debug!(block_cache_stats = ?BlockCache::stats());
}

#[test_case]
fn max_name_len() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();
    let directory = make_file(&mut fs, Kind::Directory);

    let name = str::from_utf8(&[b'x'; MAX_NAME_LEN]).unwrap();
    let file = fs.insert(&directory, name, Kind::File).unwrap();

    assert!(fs.insert(&directory, name, Kind::File).is_err());

    let list = fs.list(&directory).unwrap();
    debug!(?list);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name().len(), MAX_NAME_LEN);
    assert_eq!(list[0].name(), name);

    fs.remove(&file).unwrap();

    let name = str::from_utf8(&[b'x'; MAX_NAME_LEN + 1]).unwrap();
    assert_eq!(
        fs.insert(&directory, name, Kind::File).unwrap_err(),
        InvalidArgument,
    );

    debug!(block_cache_stats = ?BlockCache::stats());
}

const CACHE_BLOCK_COUNT: usize = 1 << 10;
const FS_DISK: usize = 1;
const RESOLVE_CACHE_SIZE: usize = 5;
