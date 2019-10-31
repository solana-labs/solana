#!/usr/bin/env bash
#
# Reports network bandwidth usage
#
set -e

cd "$(dirname "$0")"

cat "$1" | awk '{ if ($3 ~ "=>") { print $2, $7 } else if ($2 ~ "<=") { print $1, $6 }} ' \
  | awk 'NR%2{printf "%s ",$0;next;}1' \
  | awk '{ print "{ \"a\": \""$1"\", " "\"b\": \""$3"\", \"a_to_b\": \""$2"\", \"b_to_a\": \""$4"\"}," }' > "$2"

if [ "$#" -ne 3 ]; then
  solana-network-tool -i "$2"
else
  solana-network-tool -i "$2" map-IP --priv `hostname -i` --pub "$3"
fi

exit 1
