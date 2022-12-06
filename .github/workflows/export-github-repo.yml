name: export-github-repo

on:
  push:
    branches:
      - master
    paths:
      - "web3.js/**"

env:
  GITHUB_TOKEN: ${{secrets.PAT}}

jobs:
  web3:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0
      - uses: actions/setup-python@v4
        with:
          python-version: "3.x"
      - name: cmd
        run: |
          chmod +x ./ci/export-github-repo.sh
          ./ci/export-github-repo.sh web3.js/ solana-web3.js
        shell: bash

  error_reporting:
    needs:
      - web3
    if: failure() && github.event_name == 'push'
    uses: ./.github/workflows/error-reporting.yml
    secrets:
      WEBHOOK: ${{ secrets.SLACK_ERROR_REPORTING_WEBHOOK }}
