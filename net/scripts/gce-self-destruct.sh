#!/usr/bin/env bash

__gce_sd_here="$(dirname "${BASH_SOURCE[0]}")"
# shellcheck disable=SC1091
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
  declare timeout_hrs=$1
  timeout_sec=$((timeout_hrs * 60 * 60))
  now_unix=$(date +%s)
  echo $((now_unix + timeout_sec))
}

relative_timespan()
{
  declare timeSpan="$1"
  declare -a units divs
  units+=( s ); divs+=( 60 )
  units+=( m ); divs+=( 60 )
  units+=( h ); divs+=( 24 )
  units+=( d ); divs+=( 7 )
  units+=( w ); divs+=( 52 )
  numUnits="${#units[@]}"
  units+=( y ); divs+=( 100 )

  declare -a outs
  declare i div remain
  for (( i=0; i < "$numUnits"; i++ )); do
    div="${divs[$i]}"
    [[ "$timeSpan" -lt "$div" ]] && break
    remain="$((timeSpan % div))"
    timeSpan="$((timeSpan / div))"
    outs+=( "$remain" )
  done
  outs+=( "$timeSpan" )

  numOut="${#outs[@]}"
  out1="$((numOut-1))"
  out2="$((numOut-2))"

  if [[ "$numOut" -eq 1 ]] || \
    [[ "$numOut" -ge "$numUnits" && \
      "${outs[out1]}" -ge "${divs[out1]}" ]]; then
    printf "%d%s" "${outs[out1]}" "${units[out1]}"
  else
    printf "%d%s%02d%s" "${outs[out1]}" \
      "${units[out1]}" "${outs[out2]}" "${units[out2]}"
  fi
}

gce_self_destruct_ttl() {
  declare colorize="${1:-true}"
  declare prefix="${2}"
  declare suffix="${3}"
  declare output=0

  if [[ -f "${__gce_sd_conf}" ]]; then
    # shellcheck disable=SC1090
    source "${__gce_sd_conf}"
    declare ttl pttl color
    ttl="$((destruct - $(date +%s)))"
    if [[ "$ttl" -lt 0 ]]; then
      ttl=0
    fi
    pttl="$(relative_timespan "$ttl")"
    color=
    if [[ ttl -lt 3600 ]]; then
      color="\033[01;31m"
    fi
    output="${prefix}${pttl}${suffix}"
    if $colorize; then
      output="${color}${output}\033[01;00m"
    fi
  fi
  echo -e "$output"
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
bash -i <<'EOF2'
source /solana-scratch/gce-self-destruct.sh
gce_self_destruct_check
EOF2
EOF
}

gce_self_destruct_check() {
  if [[ -f "${__gce_sd_conf}" ]]; then
    # shellcheck disable=SC1090
    source "${__gce_sd_conf}"
    declare now gcloudBin
    now=$(date +%s)
    if [[ "$now" -ge "$destruct" ]]; then
      # gcloud is installed in /snap/bin, but /snap/bin isn't in root's PATH...
      gcloudBin="$(command -v gcloud)"
      gcloudBin="${gcloudBin:-/snap/bin/gcloud}"
      "$gcloudBin" compute instances delete --quiet "$(hostname)" --zone "$zone"
    else
      at -t "$(unix_to_at_time "$destruct")" <<EOF
bash -i <<'OEF2'
source /solana-scratch/gce-self-destruct.sh
gce_self_destruct_check
EOF2
EOF
    fi
  fi
}

gce_self_destruct_motd() {
  declare ttl
  ttl="$(gce_self_destruct_ttl)"
  echo -e '\n~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\n'
  if [[ -n "${ttl}" ]]; then
    echo -e "\tThis instance will self-destruct in ${ttl}!"
  else
    echo -e "\tThis instance will NOT self-destruct. YOU are responsible for deleting it!"
  fi
  echo -e '\n~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\n'
}

gce_self_destruct_ps1() {
  declare ttl
  ttl="$(gce_self_destruct_ttl true "[T-" "]")"
  ttl="${ttl:-"[T-~~~~~]"}"
  echo "!${ttl}"
}

