name: Web3.js Experimental

on:
  push:
    branches: ["master"]
    paths:
      - "web3.js-experimental/**"
      - ".github/workflows/web3-experimental.yml"
  pull_request:
    types: [opened, synchronize]
    paths:
      - "web3.js-experimental/**"
      - ".github/workflows/web3-experimental.yml"

jobs:
  build:
    name: Build
    timeout-minutes: 15
    runs-on: ubuntu-latest

    defaults:
      run:
        working-directory: web3.js-experimental

    # To use Turborepo Remote Caching, set the following environment variables for the job.
    env:
      TURBO_TOKEN: ${{ secrets.TURBO_TOKEN }}
      TURBO_TEAM: ${{ secrets.TURBO_TEAM }}

    steps:
      - name: Check out code
        uses: actions/checkout@v3
        with:
          fetch-depth: 2

      - uses: pnpm/action-setup@v2.2.4
        with:
          version: 7

      - name: Setup Node.js environment
        uses: actions/setup-node@v3
        with:
          node-version: 16
          cache: "pnpm"
          cache-dependency-path: "web3.js-experimental/pnpm-lock.yaml"

      - name: Install dependencies
        run: pnpm install

      - name: Build
        run: pnpm build

      - name: Upload build artifacts
        uses: actions/upload-artifact@v3
        with:
          name: dist
          path: |
            ./web3.js-experimental/dist/
            ./web3.js-experimental/package.json
