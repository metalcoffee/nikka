#!/usr/bin/env bash

test=$1

if [ -z "$test" ]; then
    echo "Usage:"
    echo "$0 [test-name]"
    echo "Eg.: $0 3-smp-5-ap-init"
    exit 1
fi

(
    cd kernel 2>/dev/null || true; \
    cargo test --no-run --test $test >/dev/null 2>/dev/null
)

echo "Waiting for GDB to attach. Run 'make gdb' in a separate console."
echo

(
    cd kernel 2>/dev/null || true; \
    cargo test --test $test -- -gdb tcp::1234 -S
)
