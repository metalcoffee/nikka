#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]
#![no_main]
#![no_std]

use heapless::{
    String,
    Vec,
};

use ku::{
    error::Result,
    log::info,
    memory::{
        Block,
        FULL_ACCESS,
        Page,
        Virt,
        mmu::{
            PAGE_OFFSET_BITS,
            PAGE_TABLE_INDEX_BITS,
            PAGE_TABLE_LEAF_LEVEL,
            PAGE_TABLE_ROOT_LEVEL,
        },
    },
    process::{
        Pid,
        State,
    },
};

use lib::{
    entry,
    memory,
    syscall,
};

entry!(main);

fn main() {
    let mut pedigree = Vec::<Pid, DEPTH>::new();

    let mut name = String::<MAX_NAME>::new();
    name.push_str("eager_fork ").unwrap();

    fork_tree(&mut pedigree, &mut name, '*');
}

fn fork_tree(
    pedigree: &mut Vec<Pid, DEPTH>,
    name: &mut String<MAX_NAME>,
    suffix: char,
) {
    name.push(suffix).unwrap();
    pedigree.push(ku::process_info().pid()).unwrap();

    info!(
        name = name.as_str(),
        ?pedigree,
        len = pedigree.len(),
        capacity = pedigree.capacity(),
    );

    let mut is_child = false;
    let mut suffix = 'x';

    for child in '0' .. '3' {
        if pedigree.len() < pedigree.capacity() {
            is_child = eager_fork().expect("failed to eager_fork()");
        }
        if is_child {
            suffix = child;
            break;
        }
    }

    if is_child {
        fork_tree(pedigree, name, suffix);
    }
}

fn eager_fork() -> Result<bool> {
    // TODO: your code here.
    unimplemented!();
}

// ANCHOR: copy_address_space
fn copy_address_space(child: Pid) -> Result<()> {
    copy_page_table(child, PAGE_TABLE_ROOT_LEVEL, Virt::default())
}
// ANCHOR_END: copy_address_space

// ANCHOR: copy_page_table
fn copy_page_table(
    child: Pid,
    level: u32,
    virt: Virt,
) -> Result<()> {
    // ANCHOR_END: copy_page_table
    // TODO: your code here.
    unimplemented!();
}

const DEPTH: usize = 3;
const MAX_NAME: usize = 64;
