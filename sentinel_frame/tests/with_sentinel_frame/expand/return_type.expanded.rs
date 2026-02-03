use sentinel_frame::with_sentinel_frame;
pub fn default() {
    extern "C" fn default_inner() {
        {}
    }
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym default_inner, clobber_abi("C")
        );
    }
}
pub(super) fn explicit_singleton() -> () {
    extern "C" fn explicit_singleton_inner() -> () {
        {}
    }
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym explicit_singleton_inner, clobber_abi("C")
        );
    }
}
fn integer(
    a: usize,
    b: usize,
) -> usize {
    extern "C" fn integer_inner(
        a: usize,
        b: usize,
    ) -> usize {
        { a + b }
    }
    let mut output: usize;
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym integer_inner, in ("rdi") a, in ("rsi") b, lateout("rax") output,
            clobber_abi("C")
        );
    }
    output
}
fn never() -> ! {
    extern "C" fn never_inner() -> ! {
        { loop {} }
    }
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym never_inner, clobber_abi("C"), options(noreturn)
        );
    }
}
