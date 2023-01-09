#
# This file maintains the rust versions for use by CI.
#
# Obtain the environment variables without any automatic toolchain updating:
#   $ source ci/rust-version.sh
#
# Obtain the environment variables updating both stable and nightly, only stable, or
# only nightly:
#   $ source ci/rust-version.sh all
#   $ source ci/rust-version.sh stable
#   $ source ci/rust-version.sh nightly

# Then to build with either stable or nightly:
#   $ cargo +"$rust_stable" build
#   $ cargo +"$rust_nightly" build
#

if [[ -n $RUST_STABLE_VERSION ]]; then
  stable_version="$RUST_STABLE_VERSION"
else
  # read rust version from rust-toolchain.toml file
  base="$(dirname "${BASH_SOURCE[0]}")"
  # pacify shellcheck: cannot follow dynamic path
  # shellcheck disable=SC1090,SC1091
  source "$base/../scripts/read-cargo-variable.sh"
  stable_version=$(readCargoVariable channel "$base/../rust-toolchain.toml")
fi

if [[ -n $RUST_NIGHTLY_VERSION ]]; then
  nightly_version="$RUST_NIGHTLY_VERSION"
else
  nightly_version=2022-11-02
fi


export rust_stable="$stable_version"
export rust_stable_docker_image=solanalabs/rust:"$stable_version"

export rust_nightly=nightly-"$nightly_version"
export rust_nightly_docker_image=solanalabs/rust-nightly:"$nightly_version"

[[ -z $1 ]] || (

  rustup_install() {
    declare toolchain=$1
    if ! cargo +"$toolchain" -V > /dev/null; then
      echo "$0: Missing toolchain? Installing...: $toolchain" >&2
      rustup install "$toolchain"
      cargo +"$toolchain" -V
    fi
  }

  set -e
  cd "$(dirname "${BASH_SOURCE[0]}")"
  case $1 in
  stable)
     rustup_install "$rust_stable"
     ;;
  nightly)
     rustup_install "$rust_nightly"
    ;;
  all)
     rustup_install "$rust_stable"
     rustup_install "$rust_nightly"
    ;;
  *)
    echo "$0: Note: ignoring unknown argument: $1" >&2
    ;;
  esac
)
