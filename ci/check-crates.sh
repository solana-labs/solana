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

error_count=0

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
            ((error_count++))
            echo "❌ ERROR"
            printf "%s\n" "${owners[@]}"
            break
        fi

        owner_id="$(echo "$owner" | awk '{print $1}')"
        if [[ ${verified_crate_owners[$owner_id]} ]]; then
            echo "✅ $owner"
        else
            echo "❌ $owner"
            ((error_count++))
        fi
    done

    echo ""
done

if [ "$error_count" -eq 0 ]; then
    exit 0
else
    exit 1
fi
