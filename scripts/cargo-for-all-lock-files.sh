#!/usr/bin/env bash

set -e

shifted_args=()
while [[ -n $1 ]]; do
  if [[ $1 = -- ]]; then
    escape_marker=found
    shift
    break
  else
    shifted_args+=("$1")
    shift
  fi
done

# When "--" appear at the first and shifted_args is empty, consume it here
# to unambiguously pass and use any other "--" for cargo
if [[ -n $escape_marker && ${#shifted_args[@]} -gt 0 ]]; then
  files="${shifted_args[*]}"
  for file in $files; do
    if [[ $file = "${file%Cargo.lock}" ]]; then
      echo "$0: unrecognizable as Cargo.lock path (prepend \"--\"?): $file" >&2
      exit 1
    fi
  done
  shifted_args=()
else
  files="$(git ls-files :**Cargo.lock | grep -vE 'programs/(librapay|move_loader)')"
fi

for lock_file in $files; do
  (
    set -x
    cd "$(dirname "$lock_file")"
    cargo "${shifted_args[@]}" "$@"
  )
done
