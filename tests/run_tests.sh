#!/usr/bin/env bash
#
# Test runner for theshfmt.
#
# Each test case is a pair of files under tests/cases/:
#   <name>.in       the input script
#   <name>.expected the expected output after `theshfmt -a`
#
# The runner copies <name>.in to a temp file, runs `theshfmt -a` on it, and
# compares the result against <name>.expected.

set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
fmt="$root/theshfmt"
cases="$here/cases"

pass=0
fail=0

for in_file in "$cases"/*.in; do
    [ -e "$in_file" ] || continue
    name=$(basename "$in_file" .in)
    expected="$cases/$name.expected"
    if [ ! -f "$expected" ]; then
        printf 'MISSING EXPECTED: %s\n' "$name"
        fail=$((fail+1))
        continue
    fi

    tmp=$(mktemp)
    cp "$in_file" "$tmp"
    NO_COLOR=1 "$fmt" -a "$tmp" >/dev/null 2>&1

    if diff -u "$expected" "$tmp" >/tmp/theshfmt_diff.$$ 2>&1; then
        printf 'PASS  %s\n' "$name"
        pass=$((pass+1))
    else
        printf 'FAIL  %s\n' "$name"
        cat /tmp/theshfmt_diff.$$
        fail=$((fail+1))
    fi
    rm -f "$tmp" /tmp/theshfmt_diff.$$
done

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
