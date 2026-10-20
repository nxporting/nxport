#!/bin/sh
# tools/build_movie_shim.sh - builds target/movie_shim.dylib (tools/movie_shim/movie_shim.c) against Homebrew's ffmpeg.
# The engine dlopens it for movie:: playback (crates/engine/src/movie.rs); without it movies fail cleanly.
set -e
cd "$(dirname "$0")/.."
mkdir -p target
clang -O2 -shared -fPIC -o target/movie_shim.dylib tools/movie_shim/movie_shim.c \
  $(pkg-config --cflags --libs libavformat libavcodec libavutil libswscale libswresample)
echo "built target/movie_shim.dylib"
