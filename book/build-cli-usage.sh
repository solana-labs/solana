#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

usage=$(cargo -q run -p solana-cli -- -C ~/.foo --help | sed 's|'"$HOME"'|~|g')

out=${1:-src/cli/usage.md}

cat src/cli/.usage.md.header > "$out"

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

in_subcommands=0
while read -r subcommand rest; do
  [[ $subcommand == "SUBCOMMANDS:" ]] && in_subcommands=1 && continue
  if ((in_subcommands)); then
      section "$(cargo -q run -p solana-cli -- help "$subcommand" | sed 's|'"$HOME"'|~|g')" "####" >> "$out"
  fi
done <<<"$usage">>"$out"
