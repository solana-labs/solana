#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

usage=$(cargo -q run -p solana-cli -- --help | sed 's|'"$HOME"'|~|g')
exec 1>& src/api-reference/cli.md

cat src/api-reference/.cli.md

printf '```text
%s
```

' "$usage"

while read subcommand rest; do
  [[ $subcommand == "SUBCOMMANDS:" ]] && in_subcommands=1
  if ((in_subcommands)); then
      printf '```text
%s
```

' "$(cargo -q run -p solana-cli -- "$subcommand" --help | sed 's|'"$HOME"'|~|g')"
  fi
done <<<"$usage"
