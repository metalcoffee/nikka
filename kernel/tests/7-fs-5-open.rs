#![deny(warnings)]
#![feature(custom_test_frameworks)]
#![feature(int_roundings)]
#![no_main]
#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(kernel::test_runner)]

extern crate alloc;

use alloc::{
    format,
    string::String,
    vec,
    vec::Vec,
};
use core::str;

use kernel::{
    Subsystems,
    fs::{
        File,
        FileSystem,
        Kind,
        test_scaffolding::BLOCK_SIZE,
    },
    log::info,
};

mod init;

init!(Subsystems::MEMORY);

#[test_case]
fn fs() {
    FileSystem::format(FS_DISK).unwrap();
    let mut fs = FileSystem::mount(FS_DISK, CACHE_BLOCK_COUNT, RESOLVE_CACHE_SIZE).unwrap();

    test_list(&mut fs, &[]);

    test_basic_operations(&mut fs);

    test_list(
        &mut fs,
        &[
            "/dir-1",
            "/dir-1/dir-2",
            "/dir-1/dir-2/dir-3",
            "/dir-1/dir-2/dir-3/file-3",
            "/dir-1/file-4",
            "/dir-1/file-5",
            "/file-1",
            "/file-2",
        ],
    );

    remove_all(&mut fs);

    test_list(&mut fs, &[]);
}

fn test_basic_operations(fs: &mut FileSystem) {
    let root = fs.open("").unwrap();

    let f1 = fs.insert(&root, "file-1", Kind::File).unwrap();
    fs.set_size(&f1, 5678).unwrap();
    fs.insert(&root, "file-to-be-erased", Kind::File).unwrap();
    let f2 = fs.insert(&root, "file-2", Kind::File).unwrap();
    fs.write(&f2, 1234, &[b'*'; 6789]).unwrap();
    let dir1 = fs.insert(&root, "dir-1", Kind::Directory).unwrap();
    fs.insert(&dir1, "file-4", Kind::File).unwrap();
    fs.insert(&dir1, "file-5", Kind::File).unwrap();
    let dir2 = fs.insert(&dir1, "dir-2", Kind::Directory).unwrap();
    let dir3 = fs.insert(&dir2, "dir-3", Kind::Directory).unwrap();
    fs.insert(&dir3, "file-3", Kind::File).unwrap();

    assert!(fs.insert(&root, "file-1", Kind::File).is_err());

    let fe = fs.open("file-to-be-erased").unwrap();
    fs.remove(&fe).unwrap();

    let mut buffer = [b'-'; 1024];

    assert!(fs.open("file-1").is_ok());
    assert!(fs.open("file-1/").is_err());
    assert!(fs.open("/file-2").is_ok());

    let f1 = fs.open("/file-1").unwrap();
    assert!(fs.read(&f1, 5675, &mut buffer[.. 16]) == Ok(3));
    assert_eq!(buffer[.. 3], [0; 3]);
    assert!(fs.read(&f1, 5679, &mut buffer[.. 16]).is_err());
    assert!(fs.read(&f1, 5678, &mut buffer[.. 16]) == Ok(0));

    let f2 = fs.open("/file-2").unwrap();
    assert!(fs.read(&f2, 1233, &mut buffer[.. 1]) == Ok(1));
    assert_eq!(buffer[0], 0);
    assert!(fs.read(&f2, 1232, &mut buffer[.. 4]) == Ok(4));
    assert_eq!(buffer[.. 4], [0, 0, b'*', b'*']);
    assert!(fs.set_size(&f2, 1232).is_ok());
    assert!(fs.set_size(&f2, 9876).is_ok());
    assert!(fs.read(&f2, 1232, &mut buffer[.. 4]) == Ok(4));
    assert_eq!(buffer[.. 4], [0; 4]);
    assert!(fs.read(&f2, 1232, &mut buffer[.. 4]) == Ok(4));
    assert!(fs.open("/dir-1").is_ok());
    assert!(fs.open("/dir-1/").is_ok());
    assert!(fs.open("/dir-1/file-4").is_ok());
    assert!(fs.open("/dir-1/file-5").is_ok());
    assert!(fs.open("/dir-1/file-5/").is_err());
    assert!(fs.open("/dir-1/dir-2").is_ok());
    assert!(fs.open("/dir-1/dir-2/dir-3").is_ok());
    assert!(fs.open("/dir-1/dir-2/dir-3/").is_ok());
    assert!(fs.open("/dir-1/dir-2/dir-3/file-3").is_ok());
    assert!(fs.open("file-to-be-erased").is_err());
    assert!(fs.open("no-such-file").is_err());
    assert!(fs.open("no-such-dir/file").is_err());
}

fn test_list(
    fs: &mut FileSystem,
    expected: &[&str],
) {
    let mut actual: Vec<_> = vec![];

    let root = fs.open("").unwrap();
    let usage = build_list(fs, &root, "", &mut actual);

    introsort::sort(&mut actual);
    assert_eq!(actual, expected);

    if actual.is_empty() {
        assert_eq!(usage, 0);
        assert_eq!(fs.used_space(), 0);
    }

    assert!(usage <= fs.used_space() + 5678_usize.next_multiple_of(BLOCK_SIZE));
    assert!(fs.used_space() <= 4 * usage);
}

fn build_list(
    fs: &mut FileSystem,
    inode: &File,
    directory_path: &str,
    list: &mut Vec<String>,
) -> usize {
    let mut usage = 0;

    for entry in fs.list(inode).unwrap() {
        let name = entry.name();
        let path = format!("{directory_path}/{name}");
        info!(path = %&path, %entry);
        list.push(path.clone());
        usage += entry.size().next_multiple_of(BLOCK_SIZE);
        if entry.kind() == Kind::Directory {
            let directory = fs.open(&path).unwrap();
            build_list(fs, &directory, &path, list);
        }
    }

    usage
}

fn remove_all(fs: &mut FileSystem) {
    let root = fs.open("").unwrap();
    remove_recursive(fs, &root, "");
    fs.set_size(&root, 0).unwrap();
}

fn remove_recursive(
    fs: &mut FileSystem,
    inode: &File,
    directory_path: &str,
) {
    for entry in fs.list(inode).unwrap() {
        let name = entry.name();
        let path = format!("{directory_path}/{name}");
        info!(path = %&path, %entry, "removing");
        let file = fs.open(&path).unwrap();
        if entry.kind() == Kind::Directory {
            remove_recursive(fs, &file, &path);
        }
        fs.remove(&file).unwrap();
    }
}

const CACHE_BLOCK_COUNT: usize = 1 << 10;
const FS_DISK: usize = 1;
const RESOLVE_CACHE_SIZE: usize = 5;
