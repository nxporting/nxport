#!/bin/bash
# tools/ios/build_movie_shim_ios.sh [OUT_PATH] [--force]
# Cross-compiles movie_shim.dylib for iOS (arm64-apple-ios16.0) statically linked with FFmpeg.
# Backs the engine's movie:: HLE (crates/engine/src/movie.rs) for in-game WebM/MP4 playback.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/target/aarch64-apple-ios/release/movie_shim.dylib}"
FORCE=0
for arg in "$@"; do
    if [ "$arg" = "--force" ]; then
        FORCE=1
    fi
done

if [ "$FORCE" -eq 0 ] && [ -f "$OUT" ] && [ "$OUT" -nt "$ROOT/tools/movie_shim/movie_shim.c" ]; then
    echo "[+] iOS movie shim up to date: $OUT"
    exit 0
fi

CACHE_DIR="/tmp/nxport_ffmpeg_ios"
FF_VERSION="7.1.1"
FF_DIR="$CACHE_DIR/ffmpeg-$FF_VERSION"
FF_TAR="$CACHE_DIR/ffmpeg-$FF_VERSION.tar.xz"
FF_URL="https://ffmpeg.org/releases/ffmpeg-$FF_VERSION.tar.xz"

mkdir -p "$CACHE_DIR"
mkdir -p "$(dirname "$OUT")"

SDK_PATH="$(xcrun --sdk iphoneos --show-sdk-path)"
CC="$(xcrun --sdk iphoneos -f clang)"

if [ ! -f "$FF_DIR/libavformat/libavformat.a" ] || [ "$FORCE" -eq 1 ]; then
    if [ ! -f "$FF_TAR" ]; then
        echo "[+] Downloading FFmpeg $FF_VERSION..."
        curl -L "$FF_URL" -o "$FF_TAR"
    fi
    # Also when the directory exists without its sources (an interrupted earlier extraction left it empty).
    if [ ! -x "$FF_DIR/configure" ]; then
        echo "[+] Extracting FFmpeg $FF_VERSION..."
        tar -xf "$FF_TAR" -C "$CACHE_DIR"
    fi

    echo "[+] Configuring FFmpeg for iOS (arm64-apple-ios16.0)..."
    (
        cd "$FF_DIR"
        ./configure \
            --enable-cross-compile \
            --target-os=darwin \
            --arch=arm64 \
            --cc="$CC" \
            --sysroot="$SDK_PATH" \
            --extra-cflags="-target arm64-apple-ios16.0 -isysroot $SDK_PATH -O2 -fPIC" \
            --extra-ldflags="-target arm64-apple-ios16.0 -isysroot $SDK_PATH" \
            --disable-programs \
            --disable-doc \
            --disable-everything \
            --disable-avdevice \
            --disable-avfilter \
            --disable-network \
            --enable-static \
            --disable-shared \
            --enable-demuxer=matroska,mov \
            --enable-decoder=vp9,vp8,vorbis,opus,h264,hevc,aac \
            --enable-parser=vp9,vp8,vorbis,opus,h264,hevc,aac \
            --enable-protocol=file \
            --enable-swscale \
            --enable-swresample
    )

    echo "[+] Building FFmpeg static libraries..."
    make -C "$FF_DIR" -j"$(sysctl -n hw.ncpu)"
fi

echo "[+] Linking $OUT..."
$CC -target arm64-apple-ios16.0 -isysroot "$SDK_PATH" -O2 -dynamiclib -fPIC \
    -install_name @rpath/movie_shim.dylib \
    -I"$FF_DIR" \
    "$ROOT/tools/movie_shim/movie_shim.c" \
    "$FF_DIR/libavformat/libavformat.a" \
    "$FF_DIR/libavcodec/libavcodec.a" \
    "$FF_DIR/libswscale/libswscale.a" \
    "$FF_DIR/libswresample/libswresample.a" \
    "$FF_DIR/libavutil/libavutil.a" \
    -lz -lbz2 -liconv \
    -framework CoreFoundation \
    -framework CoreMedia \
    -framework CoreVideo \
    -framework VideoToolbox \
    -framework AudioToolbox \
    -framework Security \
    -o "$OUT"

codesign -f -s - "$OUT"

echo "[+] Built iOS movie shim: $OUT ($(stat -f %z "$OUT" | awk '{printf "%.1f MB", $1/1048576}'))"
