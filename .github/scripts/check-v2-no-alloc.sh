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
done < <(find tests-v2/programs -name Cargo.toml -type f | sort)

echo "audited $checked_fixtures allocator-free V2 fixtures"
echo "deferred $deferred_spl_fixtures SPL fixtures and $deferred_v1_fixtures V1 fixtures"

# These declarations are either host-gated compatibility or test-only code.
# Any declaration in another runtime source file must be classified explicitly.
while IFS= read -r occurrence; do
    [[ -z "$occurrence" ]] && continue
    source_file="${occurrence%%:*}"
    case "$source_file" in
        lang-v2/src/lib.rs | lang-v2/src/instruction.rs)
            echo "classified host-gated allocator declaration: $occurrence"
            ;;
        lang-v2/src/pod.rs)
            echo "classified test-only allocator declaration: $occurrence"
            ;;
        *)
            echo "unclassified allocator reference: $occurrence" >&2
            audit_status=1
            ;;
    esac
done < <(
    grep -RInE --include='*.rs' 'extern crate alloc|alloc::' lang-v2/src \
        | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//(/|!)?' \
        || true
)

exit "$audit_status"
