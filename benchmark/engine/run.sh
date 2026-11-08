#!/usr/bin/env bash
set -euo pipefail
mode="${1:-default}"
command="${2:-build}"
case "$mode" in default|resident) ;; *) echo 'usage: run.sh default|resident build|bench|smoke [NEW_DIRECTORY]' >&2; exit 2;; esac
case "$command" in build|bench|smoke) ;; *) echo 'expected build, bench or smoke' >&2; exit 2;; esac
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/benchmark/target/engine-$mode}"
args=(+1.94.0 build --release --locked --manifest-path "$root/benchmark/engine/Cargo.toml")
if [[ "$mode" == resident ]]; then args+=(--features resident); fi
cargo "${args[@]}"
if [[ "$command" != build ]]; then
  [[ $# == 3 ]] || { echo 'bench/smoke requires a new output directory' >&2; exit 2; }
  exec "$CARGO_TARGET_DIR/release/ulmdb-engine-benchmark" "$command" "$3"
fi
