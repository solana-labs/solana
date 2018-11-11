#!/usr/bin/env bash
set -e

if [[ $(uname) != Linux ]]; then
  echo Performance libraries are only available for Linux
  exit 1
fi

if [[ $(uname -m) != x86_64 ]]; then
  echo Performance libraries are only available for x86_64 architecture
  exit 1
fi

mkdir -p target/perf-libs
(
  cd target/perf-libs
  (
    set -x
    curl https://solana-perf.s3.amazonaws.com/v0.10.5/x86_64-unknown-linux-gnu/solana-perf.tgz | tar zxvf -
  )

  : "${CUDA_HOME:=/usr/local/cuda}"

  if [[ -r "$CUDA_HOME"/version.txt && -r cuda-version.txt ]]; then
    if ! diff "$CUDA_HOME"/version.txt cuda-version.txt > /dev/null; then
        echo ==============================================
        echo "Warning: possible CUDA version mismatch with $CUDA_HOME"
        echo
        echo "Expected version: $(cat cuda-version.txt)"
        echo "Detected version: $(cat "$CUDA_HOME"/version.txt)"
        echo ==============================================
    fi
  else
    echo ==============================================
    echo Warning: unable to validate CUDA version
    echo ==============================================
  fi

  echo "Downloaded solana-perf version: $(cat solana-perf-HEAD.txt)"
)

exit 0
