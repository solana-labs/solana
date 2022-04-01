#!/usr/bin/env bash

# Convert .bob and .msc files in docs/art to .svg files located where the
# site build will find them.

set -e

cd "$(dirname "$0")"
output_dir=static/img

svgbob_cli="$(command -v svgbob_cli || true)"
if [[ -z "$svgbob_cli" ]]; then
  svgbob_cli="$(command -v svgbob || true)"
  [[ -n "$svgbob_cli" ]] || ( echo "svgbob_cli binary not found" && exit 1 )
fi

mkdir -p "$output_dir"

while read -r bob_file; do
  out_file=$(basename "${bob_file%.*}".svg)
  "$svgbob_cli" "$bob_file" --output "$output_dir/$out_file"
done < <(find art/*.bob)

while read -r msc_file; do
  out_file=$(basename "${msc_file%.*}".png)
  mscgen -T png -o "$output_dir/$out_file" -i "$msc_file"
done < <(find art/*.msc)
