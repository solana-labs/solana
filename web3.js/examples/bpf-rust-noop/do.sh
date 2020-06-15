#!/usr/bin/env bash

cd "$(dirname "$0")"

usage() {
    cat <<EOF

Usage: do.sh action <project>

If relative_project_path is ommitted then action will
be performed on all projects

Supported actions:
    build
    clean
    test
    clippy
    fmt

EOF
}

sdkDir=../../bpf-sdk
targetDir="$PWD"/target
profile=bpfel-unknown-unknown/release

perform_action() {
    set -e
    case "$1" in
    build)
        "$sdkDir"/rust/build.sh "$PWD"

        so_path="$targetDir/$profile"
        so_name="solana_bpf_rust_noop"
        if [ -f "$so_path/${so_name}.so" ]; then
            cp "$so_path/${so_name}.so" "$so_path/${so_name}_debug.so"
            "$sdkDir"/dependencies/llvm-native/bin/llvm-objcopy --strip-all "$so_path/${so_name}.so" "$so_path/$so_name.so"
        fi
        ;;
    clean)
        "$sdkDir"/rust/clean.sh "$PWD"
        ;;
    test)
        echo "test"
        cargo +nightly test
        ;;
    clippy)
        echo "clippy"
        cargo +nightly clippy
        ;;
    fmt)
        echo "formatting"
        cargo fmt
        ;;
    help)
        usage
        exit
        ;;
    *)
        echo "Error: Unknown command"
        usage
        exit
        ;;
    esac
}

set -e

perform_action "$1"
