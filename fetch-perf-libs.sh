#!/usr/bin/env bash

PERF_LIBS_VERSION=v0.14.0

set -e
cd "$(dirname "$0")"

if [[ ! -f target/perf-libs/.$PERF_LIBS_VERSION ]]; then
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
    set -x
    cd target/perf-libs
    curl -L --retry 5 --retry-delay 2 --retry-connrefused -o solana-perf.tgz \
      https://github.com/solana-labs/solana-perf-libs/releases/download/$PERF_LIBS_VERSION/solana-perf.tgz
    tar zxvf solana-perf.tgz
    rm -f solana-perf.tgz
    touch .$PERF_LIBS_VERSION
  )
fi


write_env() {
  rm -f target/perf-libs/env.sh
  cat >> target/perf-libs/env.sh <<EOF
export CUDA_HOME="$CUDA_HOME"
export SOLANA_PERF_LIBS_CUDA="$SOLANA_PERF_LIBS_CUDA"
EOF
cat >> target/perf-libs/env.sh <<'EOF'
SOLANA_PERF_LIBS="$(cd $(dirname "${BASH_SOURCE[0]}"); pwd)"

echo "solana-perf-libs version: $(cat $SOLANA_PERF_LIBS/solana-perf-HEAD.txt)"
echo CUDA_HOME="$CUDA_HOME"

echo LD_LIBRARY_PATH="$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA:$CUDA_HOME/lib64:$LD_LIBRARY_PATH"
export LD_LIBRARY_PATH="$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA:$CUDA_HOME/lib64:$LD_LIBRARY_PATH"

echo PATH="$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA:$CUDA_HOME/bin:$PATH"
export PATH="$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA:$CUDA_HOME/bin:$PATH"

if [[ -r "$CUDA_HOME"/version.txt && -r $SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA/cuda-version.txt ]]; then
  if ! diff "$CUDA_HOME"/version.txt "$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA"/cuda-version.txt > /dev/null; then
      echo ==============================================
      echo "Warning: possible CUDA version mismatch with $CUDA_HOME"
      echo
      echo "Expected version: $(cat "$SOLANA_PERF_LIBS/$SOLANA_PERF_LIBS_CUDA"/cuda-version.txt)"
      echo "Detected version: $(cat "$CUDA_HOME"/version.txt)"
      echo ==============================================
  fi
else
  echo ==============================================
  echo Warning: unable to validate CUDA version
  echo ==============================================
fi
EOF
  echo
  echo "source $PWD/target/perf-libs/env.sh to setup environment"
  exit 0
}


for cuda in $(cd target/perf-libs; find . -maxdepth 1 -type d -regex './cuda-.*' | sort -r); do
  cuda=$(basename "$cuda")
  CUDA_HOME=/usr/local/$cuda
  SOLANA_PERF_LIBS_CUDA=$cuda
  [[ -d $CUDA_HOME ]] || {
    echo "$cuda not detected: $CUDA_HOME directory does not exist"
    continue
  }
  [[ -r $CUDA_HOME/version.txt ]] || {
    echo "$cuda not detected: $CUDA_HOME/version.txt does not exist"
    continue
  }
  echo
  cat "$CUDA_HOME/version.txt"
  echo "CUDA_HOME=$CUDA_HOME"
  write_env
done

echo
echo No supported CUDA versions detected
exit 1
