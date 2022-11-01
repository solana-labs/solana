name: release-artifacts-manually

on:
  workflow_dispatch:
    inputs:
      commit:
        type: string
        required: true
        description: commit

jobs:
  release-artifacts:
    uses: ./.github/workflows/release-artifacts.yml
    with:
      commit: ${{ github.event.inputs.commit }}
    secrets:
      AWS_ACCESS_KEY_ID: ${{ secrets.AWS_ACCESS_KEY_ID }}
      AWS_SECRET_ACCESS_KEY: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
      AWS_S3_BUCKET: ${{ secrets.AWS_S3_BUCKET }}
