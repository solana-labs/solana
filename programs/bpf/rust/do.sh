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

sdkDir=../../../sdk
targetDir="$PWD"/../target
profile=bpfel-unknown-unknown/release

perform_action() {
    set -ex
    case "$1" in
    build)
         "$sdkDir"/bpf/rust/build.sh "$2"

         so_path="$targetDir/$profile/"
         so_name="solana_bpf_rust_${3%/}"
         if [ -f "$so_path/${so_name}.so" ]; then
               cp "$so_path/${so_name}.so" "$so_path/${so_name}_debug.so"
                "$sdkDir"/bpf/dependencies/llvm-native/bin/llvm-objcopy --strip-all "$so_path/${so_name}.so" "$so_path/$so_name.so"
        fi
        ;;
    clean)
         "$sdkDir"/bpf/rust/clean.sh "$2"
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
        # - rustfilt
        (
            pwd
            ./do.sh build "$3"

            cd "$3"
            so="$targetDir"/"$profile"/solana_bpf_rust_"${3%/}".so
            dump="$targetDir"/"${3%/}"-dump

            if [ -f "$so" ]; then
                ls \
                    -la \
                    "$so" \
                    > "${dump}-mangled.txt"
                greadelf \
                    -aW \
                    "$so" \
                    >> "${dump}-mangled.txt"
                ../"$sdkDir"/bpf/dependencies/llvm-native/bin/llvm-objdump \
                    -print-imm-hex \
                    --source \
                    --disassemble \
                    "$so" \
                    >> "${dump}-mangled.txt"
                sed \
                    s/://g \
                    < "${dump}-mangled.txt" \
                    | rustfilt \
                    > "${dump}.txt"
            fi
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