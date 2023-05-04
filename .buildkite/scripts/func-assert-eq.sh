#!/usr/bin/env bash

assert_eq() {
  local test_name=$1
  local want=$2
  local got=$3

  if [[ "$want" = "$got" ]]; then
    echo "✅ $test_name"
  else
    cat <<EOF
❌ $test_name
$(diff -u <(echo "$want") <(echo "$got"))
EOF
  fi
}
