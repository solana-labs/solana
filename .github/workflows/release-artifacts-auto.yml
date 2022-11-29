name: release-artifacts-auto

on:
  push:
    branches:
      - master
      - v[0-9]+.[0-9]+
    tags:
      - v[0-9]+.[0-9]+.[0-9]+

jobs:
  release-artifacts:
    uses: ./.github/workflows/release-artifacts.yml
    with:
      commit: ${{ github.sha }}
    secrets:
      AWS_ACCESS_KEY_ID: ${{ secrets.AWS_ACCESS_KEY_ID }}
      AWS_SECRET_ACCESS_KEY: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
      AWS_S3_BUCKET: ${{ secrets.AWS_S3_BUCKET }}

  error_reporting:
    needs:
      - release-artifacts
    if: failure() && github.event_name == 'push'
    uses: ./.github/workflows/error-reporting.yml
    secrets:
      WEBHOOK: ${{ secrets.SLACK_ERROR_REPORTING_WEBHOOK }}
