#!/usr/bin/env bash

# Convert .bob and .msc files in docs/art to .svg files located where the
# site build will find them.

set -e

cd "$(dirname "$0")"
output_dir=static/img

mkdir -p "$output_dir"

while read -r bob_file; do
  out_file=$(basename "${bob_file%.*}".svg)
  svgbob "$bob_file" --output "$output_dir/$out_file"
done < <(find art/*.bob)

while read -r msc_file; do
  out_file=$(basename "${msc_file%.*}".png)
  mscgen -T png -o "$output_dir/$out_file" -i "$msc_file"
done < <(find art/*.msc)
