#!/bin/bash
# tools/build_ryujinx_shader.sh - build a patched Ryujinx shader translator for tools/sass2spv.
#
# The prebuilt DLL in tools/ryujinx_extracted fails on shaders that use subroutines (an expression gets propagated into
# a call argument, which the SPIR-V backend cannot emit). This clones the Ryujinx source at a pinned commit, applies
# the patches in tools/ryujinx_patches/, builds Ryujinx.Graphics.Shader, and installs the result into
# tools/ryujinx_patched/ (git-ignored). sass2spv.csproj uses that directory automatically when it exists.
#
# Only the projects the shader translator needs are fetched (sparse checkout); nothing is vendored into this repo.
set -euo pipefail

REPO=https://github.com/NextendoNetwork/Ryujinx-Nextendo.git
PIN=fe5e4e1a1a082f41e70b37f60b0bb86c37254878   # a fork that still carries the Ryujinx source (upstream was taken down)

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="${1:-$(mktemp -d)}"
OUT="$HERE/ryujinx_patched"
export DOTNET_ROOT="${DOTNET_ROOT:-/opt/homebrew/opt/dotnet/libexec}"
export DOTNET_ROLL_FORWARD=Major

echo "[+] fetching $PIN into $WORK"
git clone --quiet --filter=blob:none --sparse "$REPO" "$WORK/src"
git -C "$WORK/src" sparse-checkout set src/Ryujinx.Graphics.Shader src/Ryujinx.Common src/Ryujinx.Graphics.GAL src/Spv.Generator
git -C "$WORK/src" checkout --quiet "$PIN"

for p in "$HERE"/ryujinx_patches/*.patch; do
    echo "[+] applying $(basename "$p")"
    git -C "$WORK/src" apply "$p"
done

echo "[+] building (this takes a few minutes)"
rm -rf "$OUT"
dotnet build "$WORK/src/src/Ryujinx.Graphics.Shader" -c Release -o "$OUT" -m:2 2>&1 | grep -E "error|Warn.*[1-9] Warn|Elapsed" || true
test -f "$OUT/Ryujinx.Graphics.Shader.dll" || { echo "[-] build failed"; exit 1; }
echo "[+] installed patched translator into $OUT"
