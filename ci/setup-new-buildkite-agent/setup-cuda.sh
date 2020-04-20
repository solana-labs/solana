#!/usr/bin/env bash

# https://developer.nvidia.com/cuda-toolkit-archive
VERSIONS=()
#VERSIONS+=("https://developer.nvidia.com/compute/cuda/10.0/Prod/local_installers/cuda_10.0.130_410.48_linux")
#VERSIONS+=("https://developer.nvidia.com/compute/cuda/10.1/Prod/local_installers/cuda_10.1.168_418.67_linux.run")
VERSIONS+=("http://developer.download.nvidia.com/compute/cuda/10.2/Prod/local_installers/cuda_10.2.89_440.33.01_linux.run")

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -xe

RUN_FILES=()
FAILED=()
for i in "${!VERSIONS[@]}"; do
  URL=${VERSIONS[$i]}
  RUN_FILE="$(basename "$URL")"
  DEST="${HERE}/${RUN_FILE}"
  if [[ -f "$DEST" ]]; then
    RUN_FILES+=( "$DEST" )
  else
    echo -ne "Downloading ${RUN_FILE}:\t"
    if wget --read-timeout=180 --tries=3 -O "$DEST" "$URL"; then
      echo "OK"
      RUN_FILES+=( "$DEST" )
    else
      echo "FAILED. Retrying..."
      FAILED+=( "$URL" )
    fi
  fi
done

if [[ 0 -ne ${#FAILED[@]} ]]; then
  for f in "${FAILED[@]}"; do
    echo "Failed to download required resource: $f"
  done
  echo "Please manually download the above resources, save them to \"${HERE}\" and rerun $0"
  exit 1
fi

apt update
apt install -y gcc make dkms

for rf in "${RUN_FILES[@]}"; do
  sh "$rf" --silent --driver --toolkit
done

# Allow normal users to use CUDA profiler
echo 'options nvidia "NVreg_RestrictProfilingToAdminUsers=0"' > /etc/modprobe.d/nvidia-enable-user-profiling.conf

# setup persistence mode across reboots
TMPDIR="$(mktemp -d)"
if pushd "$TMPDIR"; then
  tar -xvf /usr/share/doc/NVIDIA_GLX-1.0/samples/nvidia-persistenced-init.tar.bz2
  ./nvidia-persistenced-init/install.sh systemd
  popd
  rm -rf "$TMPDIR"
fi

nvidia-smi -pm ENABLED