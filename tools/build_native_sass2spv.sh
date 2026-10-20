#!/bin/bash
# The NativeAOT shader translator the engine loads at run time: tools/build_native_sass2spv.sh [macos|ios] [OUT].
# macos (default): target/native_sass2spv/; ios: target/aarch64-apple-ios/release/, where app builds look for it.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLATFORM="${1:-macos}"
case "$PLATFORM" in
    macos) RID=osx-arm64; OUT="${2:-$ROOT/target/native_sass2spv}"; EXTRA=() ;;
    ios) RID=ios-arm64; OUT="${2:-$ROOT/target/aarch64-apple-ios/release}"; EXTRA=(-p:PublishAotUsingRuntimePack=true -p:InvariantGlobalization=true) ;;
    *) OUT="$PLATFORM"; RID=osx-arm64; EXTRA=() ;;  # older form: the output directory alone
esac

export DOTNET_ROOT="${DOTNET_ROOT:-/opt/homebrew/opt/dotnet/libexec}"
export DOTNET_ROLL_FORWARD=Major

mkdir -p "$OUT"
echo "[+] Publishing the NativeAOT translator ($RID) to $OUT..."
TMP=$(mktemp -d)
dotnet publish "$SCRIPT_DIR/sass2spv/sass2spv.csproj" -c Release -r "$RID" -p:PublishAot=true -p:NativeLib=Shared ${EXTRA[@]+"${EXTRA[@]}"} -o "$TMP"
cp "$TMP/sass2spv.dylib" "$OUT/sass2spv.dylib"
[ "$RID" = osx-arm64 ] && cp "$TMP/sass2spv.dylib" "$OUT/libsass2spv.dylib"
rm -rf "$TMP"
echo "[+] Translator ready at $OUT/sass2spv.dylib"
