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
    log::{
        Level,
        info,
    },
    memory::{
        Block,
        Page,
        PageFaultInfo,
        Virt,
        mmu::{
            PAGE_OFFSET_BITS,
            PAGE_TABLE_INDEX_BITS,
            PAGE_TABLE_LEAF_LEVEL,
            PAGE_TABLE_ROOT_LEVEL,
            PageTableFlags,
            SYSCALL_ALLOWED_FLAGS,
            USER_RW,
        },
    },
    process::{
        ExitCode,
        Info,
        Pid,
        ResultCode,
        State,
        TrapInfo,
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
    name.push_str("cow_fork ").unwrap();

    let trap_stack = Block::default(); // TODO: remove before flight.
    // TODO: your code here.

    fork_tree(&mut pedigree, &mut name, '*', trap_stack);
}

fn fork_tree(
    pedigree: &mut Vec<Pid, DEPTH>,
    name: &mut String<MAX_NAME>,
    suffix: char,
    trap_stack: Block<Page>,
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
            is_child = cow_fork(trap_stack).expect("failed to cow_fork()")
        }
        if is_child {
            suffix = child;
            break;
        }
    }

    if is_child {
        fork_tree(pedigree, name, suffix, trap_stack);
    }
}

fn cow_fork(trap_stack: Block<Page>) -> Result<bool> {
    // TODO: your code here.
    unimplemented!();
}

fn copy_address_space(
    child: Pid,
    trap_stack: Block<Page>,
) -> Result<()> {
    copy_page_table(child, PAGE_TABLE_ROOT_LEVEL, trap_stack, Virt::default())
}

// ANCHOR: copy_page_table
fn copy_page_table(
    child: Pid,
    level: u32,
    trap_stack: Block<Page>,
    virt: Virt,
) -> Result<()> {
    // ANCHOR_END: copy_page_table
    // TODO: your code here.
    unimplemented!();
}

fn trap_handler(info: &TrapInfo) {
    // TODO: your code here.
    unimplemented!();
}

const DEPTH: usize = 3;
const MAX_NAME: usize = 64;
