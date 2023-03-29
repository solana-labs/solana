name: "Issue Label Actions"

on:
  issues:
    types: [labeled, unlabeled]

permissions:
  contents: read
  issues: write

jobs:
  action:
    runs-on: ubuntu-latest
    steps:
      - uses: dessant/label-actions@v2
