name: "Pull Request Labeler"
on:
  - pull_request_target # Danger; in `target` mode secrets are available to this workflow, even when the pull request comes from a community member.

jobs:
  triage:
    permissions:
      contents: read
      pull-requests: write
    runs-on: ubuntu-latest
    steps:
      - uses: actions/labeler@v4
        with:
          repo-token: "${{ secrets.GITHUB_TOKEN }}"
          sync-labels: true
