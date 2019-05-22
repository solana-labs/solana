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
    curl https://solana-perf.s3.amazonaws.com/v0.12.1/x86_64-unknown-linux-gnu/solana-perf.tgz | tar zxvf -
  )

  if [[ -r solana-perf-CUDA_HOME.txt ]]; then
    CUDA_HOME=$(cat solana-perf-CUDA_HOME.txt)
  else
    CUDA_HOME=/usr/local/cuda
  fi

  echo CUDA_HOME="$CUDA_HOME"
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

  cat > env.sh <<EOF
export CUDA_HOME=$CUDA_HOME
export LD_LIBRARY_PATH="$PWD:$CUDA_HOME/lib64:$LD_LIBRARY_PATH"
export PATH="$PATH:$CUDA_HOME/bin"
EOF

  echo "Downloaded solana-perf version: $(cat solana-perf-HEAD.txt)"
  echo
  echo "source ./target/perf-libs/env.sh to setup compatible build environment"
)

exit 0
