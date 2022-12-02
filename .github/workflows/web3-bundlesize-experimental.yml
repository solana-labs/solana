name: Web3.js Analyze Bundle Size

on:
  workflow_run:
    workflows:
      - Web3.js Experimental
    types:
      - completed

jobs:
  compare-bundle-size:
    name: Compare Bundle Size
    runs-on: ubuntu-latest

    if: >
      github.event.workflow_run.event == 'pull_request' &&
      github.event.workflow_run.conclusion == 'success'

    env:
      # Configuration for Bundlewatch
      BUNDLEWATCH_GITHUB_TOKEN: ${{ secrets.BUNDLEWATCH_GITHUB_TOKEN }}
      CI_COMMIT_SHA: ${{ github.event.pull_request.head.sha || github.sha }}

    steps:
      - uses: pnpm/action-setup@v2.2.4
        with:
          version: 7
          run_install: false

      - name: Download built bundles
        uses: dawidd6/action-download-artifact@v2
        with:
          workflow: ${{ github.event.workflow_run.workflow_id }}
          run_id: ${{github.event.workflow_run.id}}
          name: dist

      - name: Run Bundlewatch
        run: pnpx bundlewatch
