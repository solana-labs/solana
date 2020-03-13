#
# This file maintains the rust versions for use by CI.
#
# Build with stable rust, updating the stable toolchain if necessary:
#   $ source ci/rust-version.sh stable
#   $ cargo +"$rust_stable" build
#
# Build with nightly rust, updating the nightly toolchain if necessary:
#   $ source ci/rust-version.sh nightly
#   $ cargo +"$rust_nightly" build
#
# Obtain the environment variables without any automatic toolchain updating:
#   $ source ci/rust-version.sh
#

if [[ -n $RUST_STABLE_VERSION ]]; then
  stable_version="$RUST_STABLE_VERSION"
else
  stable_version=1.42.0
fi

if [[ -n $RUST_NIGHTLY_VERSION ]]; then
  nightly_version="$RUST_NIGHTLY_VERSION"
else
  nightly_version=2020-03-12
fi


export rust_stable="$stable_version"
export rust_stable_docker_image=solanalabs/rust:"$stable_version"

export rust_nightly=nightly-"$nightly_version"
export rust_nightly_docker_image=solanalabs/rust-nightly:"$nightly_version"

[[ -z $1 ]] || (

  rustup_install() {
    declare toolchain=$1
    if ! cargo +"$toolchain" -V; then
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
  *)
    echo "Note: ignoring unknown argument: $1"
    ;;
  esac
)
