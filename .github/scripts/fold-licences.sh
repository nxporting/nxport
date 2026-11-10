#!/usr/bin/env bash
# Appends the licences of the two projects nxport's crates derive from to a release's notice file (after notices.pl's
# crate notices), so an archive carries one file of third-party notices.
set -euo pipefail
out="$1"
rule=$(printf '=%.0s' {1..72})
{
    printf '\n\n%s\nsass2spv: shader translation derived from Ryujinx\n%s\n\n' "$rule" "$rule"
    cat crates/sass2spv/LICENSE
    printf '\n\n%s\nThe lifter: derived from LASRecomp\n%s\n\n' "$rule" "$rule"
    cat crates/lifter/LICENSE-LASRecomp
} >> "$out"
