#!/usr/bin/env bash
# Cross-compile the app for Windows (GNU toolchain, no MSVC) and copy endgame.exe to /win_mnt/.
# Requires: apt install gcc-mingw-w64-x86-64
set -euo pipefail

TARGET=x86_64-pc-windows-gnu
DEST=/win_mnt

cd "$(dirname "$0")/.."

command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "missing linker: apt install gcc-mingw-w64-x86-64" >&2; exit 1; }
rustup target add "$TARGET"
cargo build -p eg-app --target "$TARGET" --release
cp "target/$TARGET/release/endgame.exe" "$DEST/"
echo "copied endgame.exe to $DEST"
