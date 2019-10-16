#!/usr/bin/env bash

__gce_sd_here="$(dirname "${BASH_SOURCE[0]}")"
__gce_sd_conf="${__gce_sd_here}/gce-self-destruct.conf"

gce_metadata_req() {
  declare endpoint url
  endpoint="$1"
  url="http://metadata.google.internal/computeMetadata/v1/$endpoint"
  curl -sf -H Metadata-Flavor:Google "$url"
}

unix_to_at_time() {
  declare unix="$1"
  date --date="@$unix" "+%Y%m%d%H%M.%S"
}

timeout_to_destruct() {
  declare timeout_sec now_unix
  declare timeout_min=$1
  timeout_sec=$((timeout_min * 60))
  now_unix=$(date +%s)
  echo $((now_unix + timeout_sec))
}

gce_self_destruct_setup() {
  declare destruct at_time zone
  destruct="$(timeout_to_destruct "$1")"
  at_time="$(unix_to_at_time "$destruct")"
  zone=$(gce_metadata_req "instance/zone")
  zone=$(basename "$zone")

  cat >"${__gce_sd_conf}" <<EOF
export destruct=$destruct
export zone=$zone
EOF

  at -t "$at_time" <<EOF
source /solana-scratch/gce-self-destruct.sh
gce_self_destruct_check
EOF
}

gce_self_destruct_check() {
  # shellcheck disable=SC1090
  source "${__gce_sd_conf}"
  declare now gcloudBin
  now=$(date +%s)
  if [[ "$now" -ge "$destruct" ]]; then
    # XXX: gcloud is installed in /snap/bin, but /snap/bin isn't in root's PATH...
    gcloudBin="$(command -v gcloud)"
    gcloudBin="${gcloudBin:-/snap/bin/gcloud}"
    "$gcloudBin" compute instances delete --quiet "$(hostname)" --zone "$zone"
  else
    at -t "$(unix_to_at_time "$destruct")" <<EOF
source /solana-scratch/gce-self-destruct.sh
gce_self_destruct_check
EOF
  fi
}
