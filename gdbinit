set architecture i386:x86-64
set disassembly-flavor intel
set print asm-demangle on
target remote localhost:1234
file target/kernel/debug/kernel
