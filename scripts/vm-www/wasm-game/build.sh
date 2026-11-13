#!/usr/bin/env bash
# Builds game.wasm (committed next to this script) from src/lib.rs. Needs
# `rustup target add wasm32-unknown-unknown`. The same sources and toolchain give the same file;
# `./build.sh --check` builds from scratch and fails if the result differs from the committed one.
set -euo pipefail
cd "$(dirname "$0")"
if [ "${1:-}" = "--check" ]; then
  export CARGO_TARGET_DIR="$(mktemp -d)"
  trap 'rm -rf "$CARGO_TARGET_DIR"' EXIT
  cargo build --release --target wasm32-unknown-unknown --locked
  cmp "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/wasm_game.wasm" game.wasm
  echo "game.wasm is what the sources build ($(sha256sum game.wasm | cut -c1-16)…)"
  exit 0
fi
export CARGO_TARGET_DIR="$PWD/target"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wasm_game.wasm game.wasm
ls -l game.wasm
