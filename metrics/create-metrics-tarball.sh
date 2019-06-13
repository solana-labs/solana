#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

echo --- Creating metrics tarball
(
  set -x
  rm -rf solana-metrics/
  mkdir solana-metrics/

  COMMIT="$(git rev-parse HEAD)"

  (
    echo "commit: $COMMIT"
  ) > solana-metrics/version.yml

  cp -a metrics/scripts/* solana-metrics

  tar jvcf solana-metrics.tar.bz2 solana-metrics/
)

ls -hl "$PWD"/solana-metrics.tar.bz2
