#!/usr/bin/env bash
#
# Reports cpu and ram usage statistics
#
set -e

[[ $(uname) == Linux ]] || exit 0

# need to cd like this to avoid #SC1091
cd "$(dirname "$0")/.."
source scripts/configure-metrics.sh

while true; do
  # collect top twice because the first time is inaccurate
  top_ouput="$(top -bn2 -d1)"
  # collect the total cpu usage by subtracting idle usage from 100%
  cpu_usage=$(echo "${top_ouput}" | grep '%Cpu(s):' | sed "s/.*, *\([0-9.]*\)%* id.*/\1/" | tail -1 | awk '{print 100 - $1}')
  # collect the total ram usage by dividing used memory / total memory
  ram_total_and_usage=$(echo "${top_ouput}" | grep '.*B Mem'| tail -1 | sed "s/.*: *\([0-9.]*\)%* total.*, *\([0-9.]*\)%* used.*/\1 \2/")
  read -r total used <<< "$ram_total_and_usage"
  ram_usage=$(awk "BEGIN {print $used / $total * 100}")
  cpu_report="cpu_usage=$cpu_usage,ram_usage=$ram_usage"

  # if nvidia-smi exists, report gpu stats
  gpu_report=""
  if [ -x "$(command -v nvidia-smi)" ]; then
    mapfile -t individual_gpu_usage < <(nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total --format=csv,nounits,noheader)
    total_gpu_usage=0
    total_gpu_mem_usage=0
    num_gpus=${#individual_gpu_usage[@]}
    for entry in "${individual_gpu_usage[@]}"
    do
      read -r compute mem_used mem_total <<< "${entry//,/}"
      total_gpu_usage=$(awk "BEGIN {print $total_gpu_usage + $compute }")
      total_gpu_mem_usage=$(awk "BEGIN {print $total_gpu_mem_usage + $mem_used / $mem_total * 100}")
    done
    avg_gpu_usage=$(awk "BEGIN {print $total_gpu_usage / $num_gpus}")
    avg_gpu_mem_usage=$(awk "BEGIN {print $total_gpu_mem_usage / $num_gpus}")
    gpu_report=",avg_gpu_usage=$avg_gpu_usage,avg_gpu_mem_usage=$avg_gpu_mem_usage"
  fi

  report="${cpu_report}${gpu_report}"
  ./scripts/metrics-write-datapoint.sh "system-stats,hostname=$HOSTNAME $report"
  sleep 1
done
