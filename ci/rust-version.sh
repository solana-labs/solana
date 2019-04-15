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

export rust_stable=1.34.0
export rust_stable_docker_image=solanalabs/rust:1.34.0

export rust_nightly=nightly-2019-03-14
export rust_nightly_docker_image=solanalabs/rust-nightly:2019-03-14

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
