name: crate-check

on:
  push:
    branches:
      - master
  pull_request:
    branches:
      - master
    paths:
      - "**/Cargo.toml"
      - ".github/workflows/crate-check.yml"

jobs:
  check:
    runs-on: ubuntu-20.04
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0

      - name: Get commit range (push)
        if: ${{ github.event_name == 'push' }}
        run: |
          echo "COMMIT_RANGE=$GITHUB_SHA" >> $GITHUB_ENV

      - name: Get commit range (pull_request)
        if: ${{ github.event_name == 'pull_request' }}
        run: |
          echo "COMMIT_RANGE=${{ github.event.pull_request.base.sha }}..${{ github.event.pull_request.head.sha }}" >> $GITHUB_ENV

      - name: Setup Rust
        shell: bash
        run: |
          source ci/rust-version.sh stable
          rustup default $rust_stable

      - name: Install toml-cli
        shell: bash
        run: |
          cargo install toml-cli

      - run: |
          ci/check-crates.sh

  error_reporting:
    needs:
      - check
    if: failure() && github.event_name == 'push'
    uses: ./.github/workflows/error-reporting.yml
    secrets:
      WEBHOOK: ${{ secrets.SLACK_ERROR_REPORTING_WEBHOOK }}
