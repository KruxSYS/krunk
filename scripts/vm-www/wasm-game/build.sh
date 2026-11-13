#!/usr/bin/env bash
# Builds game.wasm (committed next to this script). Needs `rustup target add wasm32-unknown-unknown`.
set -euo pipefail
cd "$(dirname "$0")"
export CARGO_TARGET_DIR="$PWD/target"
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wasm_game.wasm game.wasm
ls -l game.wasm
