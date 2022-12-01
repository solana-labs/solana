#!/usr/bin/env bash

# env:
#   - CRATE_TOKEN

declare -A ignore_files=(
    ["./Cargo.toml"]=1
    ["./programs/sbf/Cargo.toml"]=1
)

declare -A verified_crate_owners=(
    ["solana-grimes"]=1
)

# search all Cargo.toml
readarray -t files <<<"$(find . -name "Cargo.toml")"

declare -A including_unverified_owners=()
declare -A unknown_errors=()

for file in "${files[@]}"; do

    if [[ ${ignore_files[$file]} ]]; then
        continue
    fi

    crate_name=$(grep -Poz '[\s\S]*?package\][\s\S]*?name = "\K([a-zA-Z0-9_\-]*)' "$file" | tr -d '\0')
    echo "=== $crate_name ($file) ==="

    readarray -t owners <<<"$(cargo owner --list -q "$crate_name" --token "$CRATE_TOKEN" 2>&1)"

    for owner in "${owners[@]}"; do
        if [[ -z $owner ]]; then
            continue
        fi
        if [[ $owner =~ ^error ]]; then
            unknown_errors+=(["$crate_name ($file)"]=1)
            echo "❌ ERROR"
            printf "%s\n" "${owners[@]}"
            break
        fi

        owner_id="$(echo "$owner" | awk '{print $1}')"
        if [[ ${verified_crate_owners[$owner_id]} ]]; then
            echo "✅ $owner"
        else
            including_unverified_owners+=(["$crate_name ($file)"]=1)
            echo "❌ $owner"
        fi
    done

    echo ""
done

if [ ${#including_unverified_owners[@]} -eq 0 ] && [ "${#unknown_errors[@]}" -eq 0 ]; then
    echo "success"
    exit 0
else
    echo "including unverified owner: ${#including_unverified_owners[@]}"
    printf "%s\n" "${!including_unverified_owners[@]}"
    echo ""

    echo "unknown error: ${#unknown_errors[@]}"
    printf "%s\n" "${!unknown_errors[@]}"
    echo ""

    exit 1
fi
