use sentinel_frame::with_sentinel_frame;
fn simple() -> i32 {
    extern "C" fn simple_inner() -> i32 {
        { 42 }
    }
    let mut output: i32;
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym simple_inner, lateout("rax") output, clobber_abi("C")
        );
    }
    output
}
fn with_args(
    a: i32,
    b: &i32,
    c: &mut i32,
) {
    extern "C" fn with_args_inner(
        a: i32,
        b: &i32,
        c: &mut i32,
    ) {
        {
            *c = a + *b;
        }
    }
    unsafe {
        asm!(
            "\n                push rbp\n                push 0\n                push 0\n                mov rbp, rsp\n                call {0}\n                add rsp, 16\n                pop rbp\n                ",
            sym with_args_inner, in ("rdi") a, in ("rsi") b, in ("rdx") c,
            clobber_abi("C")
        );
    }
}
