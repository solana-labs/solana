#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

# md check
find src -name '*.md' -a \! -name SUMMARY.md |
 while read -r file; do
   if ! grep -q '('"${file#src/}"')' src/SUMMARY.md; then
       echo "Error: $file missing from SUMMARY.md"
       exit 1
   fi
 done

mdbook --version
mdbook-linkcheck --version
make -j"$(nproc)"
