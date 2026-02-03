#!/usr/bin/env python3

import re
import subprocess
import sys


def symbolize(addresses, executable):
    symbolizer = 'llvm-symbolizer'

    return subprocess.run(
        [symbolizer, '--exe', executable], input='\n'.join(addresses),
        check=True, capture_output=True, text=True,
    ).stdout


def process_logs(logs_content, executable):
    outputs = []

    pattern = re.compile(
        r'''
        (?:
            Backtrace:\s*\n
            (?P<multiline>(
                (?:[ \t]*0[xv][0-9A-Fa-f]+[ \t]*\n)+
            ))
            |
            backtrace:\s*\[?
            (?P<inline>
                (?:[ \t]*0[xv][0-9A-Fa-f]+)+
            )
            \]?
        )
        ''',
        re.VERBOSE | re.MULTILINE,
    )

    for match in pattern.finditer(logs_content):
        addresses = match.group('multiline') or match.group('inline')
        addresses_list = addresses.split()

        outputs.append(symbolize(addresses_list, executable))

    return outputs


def main():
    backtraces = process_logs(
        logs_content=sys.stdin.read(),
        executable=sys.argv[1],
    )

    if backtraces:
        plural = len(backtraces) > 1
        suffix = (
            'backtrace' if not plural else
            'backtraces, printing them in order they occur in logs'
        )
        print(f'Found {len(backtraces)} {suffix}')
        for i, backtrace in enumerate(backtraces):
            print('=' * 80)
            print(f'Backtrace #{i + 1}')
            print(backtrace)


if __name__ == '__main__':
    main()
