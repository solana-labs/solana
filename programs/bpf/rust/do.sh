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

sdkDir=../../..
targetDir=../../target
profile=bpfel-unknown-unknown/release

perform_action() {
    set -e
    case "$1" in
    build)
         "$sdkDir"/sdk/bpf/rust/build.sh "$2"
        ;;
    clean)
         "$sdkDir"/sdk/bpf/rust/clean.sh "$2"
        ;;
    test)
        (
            cd "$2"
            echo "test $2"
            cargo +nightly test
        )
        ;;
    clippy)
        (
            cd "$2"
            echo "clippy $2"
            cargo +nightly clippy
        )
        ;;
    fmt)
        (
            cd "$2"
            echo "formatting $2"
            cargo fmt
        )
        ;;
    dump)
        # Dump depends on tools that are not installed by default and must be installed manually
        # - greadelf
        # - llvm-objdump
        # - rustfilt
        (
            pwd
            ./do.sh build "$3"

            cd "$3"

            ls \
                -la \
                "$targetDir"/"$profile"/solana_bpf_rust_"${3%/}".so \
                > "$targetDir"/"${3%/}"-dump-mangled.txt
            greadelf \
                -aW \
                "$targetDir"/"$profile"/solana_bpf_rust_"${3%/}".so \
                >> "$targetDir"/"${3%/}"-dump-mangled.txt
            llvm-objdump \
                -print-imm-hex \
                --source \
                --disassemble \
                "$targetDir"/"$profile"/solana_bpf_rust_"${3%/}".so \
                >> "$targetDir"/"${3%/}"-dump-mangled.txt
            sed \
                s/://g \
                < "$targetDir"/"${3%/}"-dump-mangled.txt \
                | rustfilt \
                > "$targetDir"/"${3%/}"-dump.txt
        )
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

if [ "$#" -ne 2 ]; then
    # Build all projects
    for project in */ ; do
        perform_action "$1" "$PWD/$project" "$project"
    done
else
    # Build requested project
    perform_action "$1" "$PWD/$2" "$2"
fi