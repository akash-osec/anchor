#!/usr/bin/env bash
set -euo pipefail

# Compile the default Anchor V2 on-chain runtime with core as the only
# standard-library component. Any target-reachable reference to std or alloc
# fails at the compiler boundary.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

toolchain="${V2_NO_ALLOC_TOOLCHAIN:-nightly}"
target="${V2_NO_ALLOC_TARGET:-bpfel-unknown-none}"

echo "checking anchor-lang without std or alloc ($target)"
cargo "+$toolchain" check \
    -p anchor-lang \
    --target "$target" \
    -Zbuild-std=core \
    --no-default-features
