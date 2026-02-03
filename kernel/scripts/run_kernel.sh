#!/bin/bash

TEMPFILE=$(mktemp)

bootimage runner $@ | tee $TEMPFILE

exitcode=${PIPESTATUS[0]}

# Finds everything in the output that resembles backtrace and
# feeds it into llvm-symbolizer
if command -v llvm-symbolizer &> /dev/null; then
  ./scripts/extract_backtraces.py $1 < $TEMPFILE
else
  echo "No llvm-symbolizer found, backtraces "\
       "wouldn't be symbolized automatically"
fi

rm $TEMPFILE

exit $exitcode
