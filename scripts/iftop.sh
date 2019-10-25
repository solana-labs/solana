#!/usr/bin/env bash
#
# Reports network bandwidth usage
#
set -e

[[ $(uname) == Linux ]] || exit 0

cd "$(dirname "$0")"

sudo iftop -i "$(ifconfig | grep mtu | grep -iv loopback | grep -i running | awk 'BEGIN { FS = ":" } ; {print $1}')" -nNbBP  -t \
	| awk '{ if ($3 ~ "=>") { print $2, $7 } else if ($2 ~ "<=") { print $1, $6 }} ' \
	| awk 'NR%2{printf "%s ",$0;next;}1' \
	| awk '{ print $1, "<=>", $3, ":",  $2, $4 }'

exit 1
