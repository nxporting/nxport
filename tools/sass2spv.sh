#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export DOTNET_ROOT="${DOTNET_ROOT:-/opt/homebrew/opt/dotnet/libexec}"
export DOTNET_ROLL_FORWARD=Major
exec dotnet "$SCRIPT_DIR/sass2spv/bin/Release/net10.0/sass2spv.dll" "$@"
