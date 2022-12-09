#!/usr/bin/env bash

# input:
#   env:
#     - CRATE_TOKEN
#     - COMMIT_RANGE

if [[ -z $COMMIT_RANGE ]]; then
  echo "COMMIT_RANGE should be provided"
  exit 1
fi

if ! command -v toml &>/dev/null; then
  echo "not found toml-cli"
  cargo install toml-cli
fi

declare skip_patterns=(
  "Cargo.toml"
  "programs/sbf"
)

declare -A verified_crate_owners=(
  ["solana-grimes"]=1
)

# get Cargo.toml from git diff
readarray -t files <<<"$(git diff "$COMMIT_RANGE" --diff-filter=AM --name-only | grep Cargo.toml)"
printf "%s\n" "${files[@]}"

error_count=0
for file in "${files[@]}"; do
  read -r crate_name package_publish workspace < <(toml get "$file" . | jq -r '(.package.name | tostring)+" "+(.package.publish | tostring)+" "+(.workspace | tostring)')
  echo "=== $crate_name ($file) ==="

  if [[ $package_publish = 'false' ]]; then
    echo -e "⏩ skip (package_publish: $package_publish)\n"
    continue
  fi

  if [[ "$workspace" != "null" ]]; then
    echo -e "⏩ skip (is a workspace root)\n"
    continue
  fi

  for skip_pattern in "${skip_patterns[@]}"; do
    if [[ $file =~ ^$skip_pattern ]]; then
      echo -e "⏩ skip (match skip patterns)\n"
      continue 2
    fi
  done

  result="$(cargo owner --list -q "$crate_name" --token "$CRATE_TOKEN" 2>&1)"
  if [[ $result =~ ^error ]]; then
    if [[ $result == *"Not Found"* ]]; then
      ((error_count++))
      echo "❌ new crate $crate_name not found on crates.io. you can either

1. mark it as not for publication in its Cargo.toml

    [package]
    ...
    publish = false

or

2. make a dummy publication with these steps:

  a. Create a empty crate locally with this template

    [package]
    name = \"<PACKAGE_NAME>\"
    version = \"0.0.1\"
    description = \"<PACKAGE_DESC>\"
    authors = [\"Solana Maintainers <maintainers@solana.foundation>\"]
    repository = \"https://github.com/solana-labs/solana\"
    license = \"Apache-2.0\"
    homepage = \"https://solana.com/\"
    documentation = \"https://docs.rs/<PACKAGE_NAME>\"
    edition = \"2021\"

  b. cargo publish --token <GRIMES_CRATES_IO_TOKEN>
"
    else
      ((error_count++))
      echo "❌ $result"
    fi
  else
    readarray -t owners <<<"$result"

    verified_owner_count=0
    unverified_owner_count=0
    for owner in "${owners[@]}"; do
      if [[ -z $owner ]]; then
        continue
      fi
      owner_id="$(echo "$owner" | awk '{print $1}')"
      if [[ ${verified_crate_owners[$owner_id]} ]]; then
        ((verified_owner_count++))
        echo "✅ $owner"
      else
        ((unverified_owner_count++))
        echo "❌ $owner"
      fi
    done

    if [[ ($unverified_owner_count -gt 0) ]]; then
      ((error_count++))
      echo "error: found unverified owner(s)"
    elif [[ ($verified_owner_count -le 0) ]]; then
      ((error_count++))
      echo "error: there are no verified owners"
    fi
  fi
  echo ""
done

if [ "$error_count" -eq 0 ]; then
  echo "success"
  exit 0
else
  exit 1
fi
