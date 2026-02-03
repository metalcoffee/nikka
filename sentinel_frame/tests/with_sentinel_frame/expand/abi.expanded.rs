use sentinel_frame::with_sentinel_frame;
extern "C" fn add(
    a: i32,
    b: i32,
) -> i32 {
    extern "C" fn add_inner(
        a: i32,
        b: i32,
    ) -> i32 {
        { a + b }
    }
    let mut output: i32;
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym add_inner, in ("rdi") a, in ("rsi") b, lateout("rax") output,
            clobber_abi("C")
        );
    }
    output
}
extern "C" fn never() -> ! {
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
