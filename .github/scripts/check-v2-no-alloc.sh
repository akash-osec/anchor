#!/usr/bin/env bash
set -euo pipefail

# Audit dependency graphs that can enter the default Anchor V2 SBF runtime.
# The compiler-level bpfel check remains authoritative for target-sensitive
# cfg paths; this script catches feature and dependency regressions earlier.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

sbf_target="${SBF_TARGET:-sbpf-solana-solana}"
sbf_tools_version="${SBF_TOOLS_VERSION:-v1.57}"
sbf_toolchain="${V2_NO_ALLOC_SBF_TOOLCHAIN:-}"

if [[ -z "$sbf_toolchain" ]]; then
    toolchain_suffix="-sbpf-solana-$sbf_tools_version"
    sbf_toolchain="$({ rustup toolchain list 2>/dev/null || true; } \
        | awk -v suffix="$toolchain_suffix" 'index($1, suffix) { print $1; exit }')"
fi

if [[ -n "$sbf_toolchain" ]]; then
    cargo_tree=(rustup run "$sbf_toolchain" cargo tree)
else
    platform_rustc="${SBF_RUSTC:-$HOME/.cache/solana/$sbf_tools_version/platform-tools/rust/bin/rustc}"
    if [[ ! -x "$platform_rustc" ]]; then
        echo "SBF compiler not found for $sbf_tools_version" >&2
        echo "run: cargo build-sbf --tools-version $sbf_tools_version --install-only" >&2
        exit 1
    fi
    cargo_tree=(env RUSTC="$platform_rustc" cargo tree)
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

forbidden_pattern='feature "(std|alloc)"|solana-instruction (feature|v)|solana-system-interface (feature|v)|solana-sysvar (feature|v)|solana-pubkey (feature|v)|borsh (feature|v)|spl-token-(interface|2022-interface|group-interface|metadata-interface) (feature|v)|mpl-token-metadata (feature|v)'
audit_status=0
tree_index=0

audit_tree() {
    local label="$1"
    shift
    tree_index=$((tree_index + 1))
    local tree="$tmp_dir/$tree_index.tree"

    echo "checking no-alloc SBF graph: $label"
    if ! "${cargo_tree[@]}" "$@" \
        --target "$sbf_target" \
        --edges features,no-dev,no-proc-macro \
        --prune anchor-derive-accounts >"$tree"; then
        echo "failed to resolve SBF graph: $label" >&2
        audit_status=1
        return
    fi

    if grep -En "$forbidden_pattern" "$tree"; then
        echo "forbidden allocation-bearing dependency: $label" >&2
        audit_status=1
    fi
}

audit_tree "anchor-lang (default SBF runtime)" \
    -p anchor-lang \
    --no-default-features

checked_fixtures=0
deferred_spl_fixtures=0
deferred_v1_fixtures=0
checked_fixture_manifests=()

while IFS= read -r manifest; do
    if grep -Eq 'anchor-(lang|spl)-v1' "$manifest"; then
        echo "deferred V1 equivalence fixture: $manifest"
        deferred_v1_fixtures=$((deferred_v1_fixtures + 1))
        continue
    fi

    if grep -Eq 'anchor-spl' "$manifest"; then
        echo "deferred until SPL-to-Pinocchio migration: $manifest"
        deferred_spl_fixtures=$((deferred_spl_fixtures + 1))
        continue
    fi

    if ! grep -Eq 'anchor-lang' "$manifest"; then
        continue
    fi

    audit_tree "$manifest" --manifest-path "$manifest"
    checked_fixtures=$((checked_fixtures + 1))
    checked_fixture_manifests+=("$manifest")
done < <(find tests-v2/programs -name Cargo.toml -type f | sort)

echo "audited $checked_fixtures allocator-free V2 fixtures"
echo "deferred $deferred_spl_fixtures SPL fixtures and $deferred_v1_fixtures V1 fixtures"

# Dependency features alone do not catch an unconditional allocator import in
# a fixture source file. Audit the source roots of every non-deferred fixture
# as well. Host-only Vec/String usage remains valid; the compiler-level target
# check is authoritative for those cfg-sensitive paths. These imports are
# never valid in the target-facing fixture code and would otherwise let a
# future fixture bypass this gate.
for manifest in "${checked_fixture_manifests[@]}"; do
    fixture_root="$(dirname "$manifest")/src"
    [[ -d "$fixture_root" ]] || continue

    while IFS= read -r source_file; do
        if occurrence=$(grep -nE '^[[:space:]]*(extern crate alloc;|use alloc::|alloc::)' "$source_file"); then
            echo "$occurrence" >&2
            echo "unclassified allocator import in V2 fixture: $source_file" >&2
            audit_status=1
        fi
    done < <(find "$fixture_root" -type f -name '*.rs' -print | sort)
done

# The compiler-level bpfel check above is authoritative for cfg-sensitive
# source. Keep this source check deliberately narrow: these are the runtime
# modules whose target implementation must never mention an allocator-backed
# type. Host, IDL, compatibility, and test modules contain intentional owned
# types and are excluded here; their target branches are still compiled by the
# authoritative check.
runtime_sources=(
    lang-v2/src/buffers.rs
    lang-v2/src/context.rs
    lang-v2/src/context_cpi.rs
    lang-v2/src/cpi.rs
    lang-v2/src/cursor.rs
    lang-v2/src/dispatch.rs
    lang-v2/src/fixed_vec.rs
    lang-v2/src/loader.rs
    lang-v2/src/program.rs
    lang-v2/src/system_program.rs
)

for source_file in "${runtime_sources[@]}"; do
    if [[ ! -f "$source_file" ]]; then
        echo "runtime source file is missing: $source_file" >&2
        audit_status=1
        continue
    fi

    if occurrence=$(grep -nE 'extern crate alloc|alloc::|std::(vec::Vec|string::String|boxed::Box|rc::Rc|cell::RefCell)|(^|[^[:alnum:]_])(Vec|String|Box|Rc|RefCell)<|Vec::(with_capacity|collect)' "$source_file"); then
        echo "$occurrence" >&2
        echo "allocator-backed runtime reference: $source_file" >&2
        audit_status=1
    fi
done

exit "$audit_status"
