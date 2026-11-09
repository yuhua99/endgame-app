#!/usr/bin/env bash
# Cross-compile for Windows (GNU toolchain, no MSVC) and copy the binaries to /win_mnt/:
#   endgame.exe     the app
#   ble_shell.exe   BLE shell spike that records raw responses to out.txt
# Requires: apt install gcc-mingw-w64-x86-64
set -euo pipefail

TARGET=x86_64-pc-windows-gnu
DEST=/win_mnt

cd "$(dirname "$0")/.."

command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "missing linker: apt install gcc-mingw-w64-x86-64" >&2; exit 1; }
rustup target add "$TARGET"
cargo build -p eg-app -p eg-transport --bin endgame --example ble_shell --target "$TARGET" --release
cp "target/$TARGET/release/endgame.exe" "target/$TARGET/release/examples/ble_shell.exe" "$DEST/"
echo "copied endgame.exe and ble_shell.exe to $DEST"
