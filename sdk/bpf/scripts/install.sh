#!/usr/bin/env bash

mkdir -p "$(dirname "$0")"/../dependencies
cd "$(dirname "$0")"/../dependencies

# Determine OS and architecture
unameOut="$(uname -s)"
case "${unameOut}" in
  Linux*)
    criterion_suffix=
    machine=linux;;
  Darwin*)
    criterion_suffix=
    machine=osx;;
  MINGW*)
    criterion_suffix=-mingw
    machine=windows;;
  *)
    criterion_suffix=
    machine=linux
esac

unameOut="$(uname -m)"
case "${unameOut}" in
  arm64*)
    arch=aarch64;;
  *)
    arch=x86_64
esac

# Feature flags
ENABLE_BPF=true  # Change to false to disable BPF installation

download() {
    declare url="$1"
    declare filename="$2"

    echo "Downloading from: $url"
    echo "Saving as: $filename"

    set -x
    if hash wget 2>/dev/null; then
        echo "Using wget to download..."
        wget "$url" -O "$filename" || return 1
    elif hash curl 2>/dev/null; then
        echo "Using curl to download..."
        curl -L "$url" -o "$filename" || return 1
    else
        echo "Error: Neither curl nor wget were found" >&2
        return 1
    fi
}

# Get function to handle downloading and extraction
get() {
  declare version=$1
  declare dirname=$2
  declare url=$3 

  echo "Entered get() function with parameters:"
  echo "Version: $version"
  echo "Dirname: $dirname"
  echo "URL: $url"

  declare cache_root=~/.cache/solana
  declare cache_dirname="$cache_root/$version/$dirname"
  declare cache_partial_dirname="$cache_dirname"_partial

  # Create cache directory if it does not exist
  if [[ -r $cache_dirname ]]; then
    echo "Cache found. Linking..."
    ln -sf "$cache_dirname" "$dirname" || return 1
    return 0
  fi

  # Clean up any existing partial cache
  echo "Cleaning up any existing partial cache..."
  rm -rf "$cache_partial_dirname" || return 1
  mkdir -p "$cache_partial_dirname" || return 1
  pushd "$cache_partial_dirname"

  # Call download function
  echo "Calling download function..."
  download "$url" "platform-tools-osx-x86_64.tar.bz2"

  # Check if the download completed successfully
  echo "Checking if downloaded file exists..."
  if [[ ! -f "platform-tools-osx-x86_64.tar.bz2" ]]; then
    echo "Download failed or file not found!"
    popd
    return 1
  fi

  echo "Extracting platform-tools-osx-x86_64.tar.bz2"
  tar --strip-components 1 -jxf "platform-tools-osx-x86_64.tar.bz2" || {
      echo "Extraction failed!" 
      popd
      return 1
  }

  popd
  mv "$cache_partial_dirname" "$cache_dirname" || return 1
  ln -sf "$cache_dirname" "$dirname" || return 1
}

# Install Criterion
if [[ $machine == "linux" ]]; then
  version=v2.3.3
else
  version=v2.3.2
fi

if [[ ! -e criterion-$version.md || ! -e criterion ]]; then
  set -e
  rm -rf criterion*
  
  url="https://github.com/Snaipe/Criterion/releases/download/$version/criterion-$version-$machine$criterion_suffix-x86_64.tar.bz2"
  
  echo "Attempting to get Criterion from: $url"
  
  get $version criterion "$url"
  exitcode=$?
  
  if [[ $exitcode -ne 0 ]]; then
    echo "Failed to download Criterion. Exiting with code: $exitcode"
    exit 1
  fi

  touch criterion-$version.md
fi

# Install platform tools
version=v1.41
if [[ ! -e platform-tools-$version.md || ! -e platform-tools ]]; then
    set -e
    rm -rf platform-tools*
    echo "Attempting to download platform tools: https://github.com/anza-xyz/platform-tools/releases/download/v1.41/platform-tools-osx-x86_64.tar.bz2"

    url="https://github.com/anza-xyz/platform-tools/releases/download/v1.41/platform-tools-osx-x86_64.tar.bz2"
    echo "Attempting to call get() function."
    
    get $version platform-tools "$url"
    
    exitcode=$?
    if [[ $exitcode -ne 0 ]]; then
        echo "Failed to download platform-tools. Exiting with error code: $exitcode"
        exit 1
    fi

    # Linking, if successful
    if [[ -d ~/.cache/solana/v1.41/platform-tools ]]; then
        ln -sf ~/.cache/solana/v1.41/platform-tools ~/.local/share/solana/install/releases/2.0.16/solana-release/bin/sdk/sbf/dependencies/platform-tools || {
            echo "Failed to create symbolic link for platform-tools."
            exit 1
        }
    fi

    touch platform-tools-$version.md
fi

# Final success message
echo "Script completed successfully."
exit 0
