name: Web3

on:
  push:
    branches: [ master ]
    paths:
      - "web3.js/**"
      - ".github/workflows/web3.yml"
  pull_request:
    branches: [ master ]
    paths:
      - "web3.js/**"
      - ".github/workflows/web3.yml"

jobs:
  # needed for grouping check-web3 strategies into one check for mergify
  all-web3-checks:
    runs-on: ubuntu-latest
    needs:
      - check-web3
    steps:
      - run: echo "Done"

  web3-commit-lint:
    runs-on: ubuntu-latest
    # Set to true in order to avoid cancelling other workflow jobs.
    # Mergify will still require web3-commit-lint for automerge
    continue-on-error: true

    defaults:
      run:
        working-directory: web3.js

    steps:
      - uses: actions/checkout@v3
        with:
          # maybe needed for base sha below
          fetch-depth: 0
      - uses: actions/setup-node@v3
        with:
          node-version: '16'
          cache: 'npm'
          cache-dependency-path: web3.js/package-lock.json
      - run: npm ci
      - name: commit-lint
        if: ${{ github.event_name == 'pull_request' }}
        run: bash commitlint.sh
        env:
          COMMIT_RANGE: ${{ github.event.pull_request.base.sha }}..${{ github.event.pull_request.head.sha }}

  check-web3:
    runs-on: ubuntu-latest

    defaults:
      run:
        working-directory: web3.js

    strategy:
      matrix:
        node:
          - '14'
          - '16'

    name: Node ${{ matrix.node }}
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-node@v3
        with:
          node-version: ${{ matrix.node }}
          cache: 'npm'
          cache-dependency-path: web3.js/package-lock.json
      - run: |
          scripts/test.sh
