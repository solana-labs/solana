#!/usr/bin/env bash

cd "$(dirname "$0")"/../dependencies

if [[ "$(uname)" = Darwin ]]; then
  machine=osx
else
  machine=linux
fi

download() {
  declare url="$1/$2/$3"
  declare filename=$3
  declare wget_args=(
    "$url" -O "$filename"
    "--progress=dot:giga"
    "--retry-connrefused"
    "--read-timeout=30"
  )
  declare curl_args=(
    -L "$url" -o "$filename"
  )
  if hash wget 2>/dev/null; then
    wget_or_curl="wget ${wget_args[*]}"
  elif hash curl 2>/dev/null; then
    wget_or_curl="curl ${curl_args[*]}"
  else
    echo "Error: Neither curl nor wget were found" >&2
    return 1
  fi

  set -x
  if $wget_or_curl; then
    tar --strip-components 1 -jxf "$filename" || return 1
    { set +x; } 2>/dev/null
    rm -rf "$filename"
    return 0
  fi
  return 1
}

clone() {
  declare url=$1
  declare version=$2

  rm -rf temp
  if (
    set -x
    git clone --recursive --depth 1 --single-branch --branch "$version" "$url" temp
  ); then
    (
      shopt -s dotglob nullglob
      mv temp/* .
    )
    return 0
  fi
  return 1
}

get() {
  declare version=$1
  declare dirname=$2
  declare job=$3
  declare cache_root=~/.cache/solana
  declare cache_dirname="$cache_root/$version/$dirname"
  declare cache_partial_dirname="$cache_dirname"_partial

  if [[ -r $cache_dirname ]]; then
    ln -sf "$cache_dirname" "$dirname" || return 1
    return 0
  fi

  rm -rf "$cache_partial_dirname" || return 1
  mkdir -p "$cache_partial_dirname" || return 1
  pushd "$cache_partial_dirname"

  if $job; then
    popd
    mv "$cache_partial_dirname" "$cache_dirname" || return 1
    ln -sf "$cache_dirname" "$dirname" || return 1
    return 0
  fi
  popd
  return 1
}

# Install xargo
version=0.3.22
if [[ ! -e xargo-$version.md ]] || [[ ! -x bin/xargo ]]; then
  (
    args=()
    # shellcheck disable=SC2154
    if [[ -n $rust_stable ]]; then
      args+=(+"$rust_stable")
    fi
    args+=(install xargo --version "$version" --root .)
    set -ex
    cargo "${args[@]}"
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    exit 1
  fi
  ./bin/xargo --version >xargo-$version.md 2>&1
fi

# Install Criterion
version=v2.3.2
if [[ ! -e criterion-$version.md || ! -e criterion ]]; then
  (
    set -e
    rm -rf criterion*
    job="download \
           https://github.com/Snaipe/Criterion/releases/download \
           $version \
           criterion-$version-$machine-x86_64.tar.bz2 \
           criterion"
    get $version criterion "$job"
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    exit 1
  fi
  touch criterion-$version.md
fi

# Install LLVM
version=v0.0.15
if [[ ! -e llvm-native-$version.md || ! -e llvm-native ]]; then
  (
    set -e
    rm -rf llvm-native*
    rm -rf xargo
    job="download \
           https://github.com/solana-labs/llvm-builder/releases/download \
           $version \
           solana-llvm-$machine.tar.bz2 \
           llvm-native"
    get $version llvm-native "$job"
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    exit 1
  fi
  touch llvm-native-$version.md
fi

# Install Rust-BPF
version=v0.2.5
if [[ ! -e rust-bpf-$machine-$version.md || ! -e rust-bpf-$machine ]]; then
  (
    set -e
    rm -rf rust-bpf-$machine*
    rm -rf xargo
    job="download \
           https://github.com/solana-labs/rust-bpf-builder/releases/download \
           $version \
           solana-rust-bpf-$machine.tar.bz2 \
           rust-bpf-$machine"
    get $version rust-bpf-$machine "$job"

    set -ex
    ./rust-bpf-$machine/bin/rustc --print sysroot
    set +e
    rustup toolchain uninstall bpf
    set -e
    rustup toolchain link bpf rust-bpf-$machine
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    exit 1
  fi
  touch rust-bpf-$machine-$version.md
fi

# Install Rust-BPF Sysroot sources
version=v0.12
if [[ ! -e rust-bpf-sysroot-$version.md || ! -e rust-bpf-sysroot ]]; then
  (
    set -e
    rm -rf rust-bpf-sysroot*
    rm -rf xargo
    job="clone \
           https://github.com/solana-labs/rust-bpf-sysroot.git \
           $version"
    get $version rust-bpf-sysroot "$job"
  )
  exitcode=$?
  if [[ $exitcode -ne 0 ]]; then
    exit 1
  fi
  touch rust-bpf-sysroot-$version.md
fi

exit 0
