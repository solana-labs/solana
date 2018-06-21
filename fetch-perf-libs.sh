#!/bin/bash -e

if [[ $(uname) != Linux ]]; then
  echo Performance libraries are only available for Linux
  exit 1
fi

if [[ $(uname -m) != x86_64 ]]; then
  echo Performance libraries are only available for x86_64 architecture
  exit 1
fi

(
  set -x
  curl -o solana-perf.tgz \
    https://solana-perf.s3.amazonaws.com/master/x86_64-unknown-linux-gnu/solana-perf.tgz
  tar zxvf solana-perf.tgz
)

if [[ -r /usr/local/cuda/version.txt && -r cuda-version.txt ]]; then
  if ! diff /usr/local/cuda/version.txt cuda-version.txt > /dev/null; then
    echo ==============================================
    echo Warning: possible CUDA version mismatch
    echo
    echo "Expected version: $(cat cuda-version.txt)"
    echo "Detected version: $(cat /usr/local/cuda/version.txt)"
    echo ==============================================
  fi
else
  echo ==============================================
  echo Warning: unable to validate CUDA version
  echo ==============================================
fi

echo "Downloaded solana-perf version: $(cat solana-perf-HEAD.txt)"

exit 0
