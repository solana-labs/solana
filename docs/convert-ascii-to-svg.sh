#!/usr/bin/env bash

# Convert .bob and .msc files in docs/art to .svg files located where the
# site build will find them.

set -e

cd "$(dirname "$0")"
output_dir=static/img

mkdir -p "$output_dir"

while read -r bob_file; do
  svg_file=$(basename "${bob_file%.*}".svg)
  svgbob "$bob_file" --output "$output_dir/$svg_file"
done < <(find art/*.bob)

while read -r msc_file; do
  svg_file=$(basename "${msc_file%.*}".svg)
  mscgen -T svg -o "$output_dir/$svg_file" -i "$msc_file"
done < <(find art/*.msc)
