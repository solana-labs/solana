#!/usr/bin/env bash

cd "$(dirname "$0")"/../dependencies

if [[ "$(uname)" = Darwin ]]; then
  machine=osx
else
  machine=linux
fi

download() {
  set -e
  declare url="$1/$2/$3"
  declare version=$2
  declare filename=$3
  declare dirname=$4
  declare progress=$5
  declare cache_directory=~/.cache/"$version"
  declare cache_dirname=$cache_directory/${dirname//:\//_}

  link() {
    set -e
    ln -sf "$cache_dirname" "$dirname"
    ln -sf "$cache_directory/$dirname-$version.md" "$dirname-$version.md"
  }

  if [[ -r $cache_dirname ]]; then
    link
    return 0
  fi

  declare args=(
    "$url" -O "$filename"
    "--progress=dot:$progress"
    "--retry-connrefused"
    "--read-timeout=30"
  )
  set -x
  mkdir -p "$cache_dirname"
  pushd "$cache_dirname"
  if wget "${args[@]}"; then
    tar --strip-components 1 -jxf "$filename"
    rm -rf "$filename"
    echo "$url" >"../$dirname-$version.md"
    popd
    link
    return 0
  fi
  popd
  rm -rf "$cache_dirname"
  return 1
}

clone() {
  set -e
  declare url=$1
  declare version=$2
  declare dirname=$3
  declare cache_directory=~/.cache/"$version"
  declare cache_dirname=$cache_directory/${dirname//:\//_}

  link() {
    set -e
    ln -sf "$cache_dirname" "$dirname"
    ln -sf "$cache_directory/$dirname-$version.md" "$dirname-$version.md"
  }

  if [[ -r $cache_dirname ]]; then
    link
    return 0
  fi

  set -x
  mkdir -p "$cache_directory"
  pushd "$cache_directory"
  cmd="git clone --recursive --depth 1 --single-branch --branch $version $url"
  if $cmd; then
    echo "$cmd" >"$dirname-$version.md"
    popd
    link
    return 0
  fi
  return 1
}

# Install xargo
(
  set -ex
  # shellcheck disable=SC2154
  if [[ -n $rust_stable ]]; then
    cargo +"$rust_stable" install xargo
  else
    cargo install xargo
  fi
  xargo --version >xargo.md 2>&1
)
# shellcheck disable=SC2181
if [[ $? -ne 0 ]]; then
  exit 1
fi

# Install Criterion
version=v2.3.2
if [[ ! -e criterion-$version.md ]]; then
  (
    set -e
    rm -rf criterion*
    download "https://github.com/Snaipe/Criterion/releases/download" \
      $version \
      "criterion-$version-$machine-x86_64.tar.bz2" \
      criterion \
      mega
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf criterion-$version.md
    exit 1
  fi
fi

# Install LLVM
version=v0.0.15
if [[ ! -e llvm-native-$version.md ]]; then
  (
    set -e
    rm -rf llvm-native*
    rm -rf xargo
    download "https://github.com/solana-labs/llvm-builder/releases/download" \
      $version \
      "solana-llvm-$machine.tar.bz2" \
      llvm-native \
      giga
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf llvm-native-$version.md
    exit 1
  fi
fi

# Install Rust-BPF
version=v0.2.3
if [[ ! -e rust-bpf-$version.md ]]; then
  (
    set -e
    rm -rf rust-bpf
    rm -rf rust-bpf-$machine-*
    rm -rf xargo
    download "https://github.com/solana-labs/rust-bpf-builder/releases/download" \
      $version \
      "solana-rust-bpf-$machine.tar.bz2" \
      rust-bpf \
      giga

    set -ex
    ./rust-bpf/bin/rustc --print sysroot
    set +e
    rustup toolchain uninstall bpf
    set -e
    rustup toolchain link bpf rust-bpf
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf-$version.md
    exit 1
  fi
fi

# Install Rust-BPF Sysroot sources
version=v0.12
if [[ ! -e rust-bpf-sysroot-$version.md ]]; then
  (
    set -e
    rm -rf rust-bpf-sysroot*
    rm -rf xargo
    clone "https://github.com/solana-labs/rust-bpf-sysroot.git" \
      $version \
      rust-bpf-sysroot
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    rm -rf rust-bpf-sysroot-$version.md
    exit 1
  fi
fi

exit 0
