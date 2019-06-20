#!/usr/bin/env bash

usage() {
    echo ""
    echo "  Usage: do.sh action <project>"
    echo ""
    echo "  If relative_project_path is ommitted then action will"
    echo "  be performed on all projects"
    echo ""
    echo "  Supported actions:"
    echo "    build"
    echo "    clean"
    echo "    clippy"
    echo "    fmt"
    echo ""
}

perform_action() {
    set -e
    case "$1" in
    build)
         ./../../../sdk/bpf/rust/build.sh "$2"
    ;;
    clean)
         ./../../../sdk/bpf/rust/clean.sh "$2"
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
            ./do.sh clean "$3"
            ./do.sh build "$3"

            cd "$3"

            set +e
            cp ./target/dump.txt ./targetdump-last.txt 2>/dev/null
            set -e

            ls \
                -la \
                ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"${3%/}".so \
                > ./target/dump_mangled.txt
            greadelf \
                -aW \
                ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"${3%/}".so \
                >> ./target/dump_mangled.txt
            llvm-objdump \
                -print-imm-hex \
                --source \
                --disassemble \
                ./target/bpfel-unknown-unknown/release/solana_bpf_rust_"${3%/}".so \
                >> ./target/dump_mangled.txt
            sed \
                s/://g \
                < ./target/dump_mangled.txt \
                | rustfilt \
                > ./target/dump.txt
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