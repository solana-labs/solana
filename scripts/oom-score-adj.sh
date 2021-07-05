# |source| this file
#
# Adjusts the OOM score for the specified process.  Linux only
#
# usage: oom_score_adj [pid] [score]
#
oom_score_adj() {
  declare pid=$1
  declare score=$2
  if [[ $(uname) != Linux ]]; then
    return
  fi

  echo "$score" > "/proc/$pid/oom_score_adj" || true
  declare currentScore
  currentScore=$(cat "/proc/$pid/oom_score_adj" || true)
  if [[ $score != "$currentScore" ]]; then
    echo "Failed to set oom_score_adj to $score for pid $pid (current score: $currentScore)"
  fi
}
