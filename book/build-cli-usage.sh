#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

usage=$(cargo -q run -p solana-cli -- --help | sed 's|'"$HOME"'|~|g')

out=${1:-src/api-reference/cli.md}

cat src/api-reference/.cli.md > "$out"

section() {
  declare mark=${2:-"###"}
  declare section=$1
  read name rest <<<"$section"

  printf '%s %s
' "$mark" "$name"
  printf '```text
%s
```

' "$section"
}

section "$usage" >> "$out"

in_subcommands=0
while read subcommand rest; do
  [[ $subcommand == "SUBCOMMANDS:" ]] && in_subcommands=1 && continue
  if ((in_subcommands)); then
      section "$(cargo -q run -p solana-cli -- help "$subcommand" | sed 's|'"$HOME"'|~|g')" "####" >> "$out"
  fi
done <<<"$usage">>"$out"

if [[ -n $CI ]]; then
  # In CI confirm that the cli reference doesn't need to be built
  git diff --exit-code
fi
