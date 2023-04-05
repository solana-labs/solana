#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

# shellcheck source=ci/env.sh
source ../ci/env.sh

# Get current channel
eval "$(../ci/channel-info.sh)"

# set the output file' location
out=${1:-src/cli/usage.md}

# load the usage file's header
cat src/cli/.usage.md.header > "$out"

# Skip generating the usage doc for non deployment commits of the docs
if [[ -n $CI ]]; then
  if [[ $CI_BRANCH != $EDGE_CHANNEL* ]] && [[ $CI_BRANCH != $BETA_CHANNEL* ]] && [[ $CI_BRANCH != $STABLE_CHANNEL* ]]; then
    echo "**NOTE:** The usage doc is only auto-generated during full production deployments of the docs"
    echo "**NOTE:** This usage doc is auto-generated during deployments" >> "$out"
    exit
  fi
fi

echo 'Building the solana cli from source...'

# shellcheck source=ci/rust-version.sh
source ../ci/rust-version.sh stable

: "${rust_stable:=}" # Pacify shellcheck

# pre-build with output enabled to appease Travis CI's hang check
cargo build -p solana-cli

usage=$(cargo -q run -p solana-cli -- -C ~/.foo --help | sed -e 's|'"$HOME"'|~|g' -e 's/[[:space:]]\+$//')

section() {
  declare mark=${2:-"###"}
  declare section=$1
  read -r name rest <<<"$section"

  printf '%s %s
' "$mark" "$name"
  printf '```text
%s
```

' "$section"
}

section "$usage" >> "$out"

usage=$(sed -e '/^ \{5,\}/d' <<<"$usage")

in_subcommands=0
while read -r subcommand rest; do
  [[ $subcommand == "SUBCOMMANDS:" ]] && in_subcommands=1 && continue
  if ((in_subcommands)); then
      section "$(cargo -q run -p solana-cli -- help "$subcommand" | sed -e 's|'"$HOME"'|~|g' -e 's/[[:space:]]\+$//')" "####" >> "$out"
  fi
done <<<"$usage">>"$out"
