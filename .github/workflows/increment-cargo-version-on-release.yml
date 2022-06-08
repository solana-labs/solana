name: increment-cargo-version

on:
  release:
    types: [published]

jobs:
  check_compilation:
    name: Increment cargo version
    runs-on: ubuntu-latest
    steps:
      - name: Checkout Repository
        uses: actions/checkout@v3

      # This script confirms two assumptions:
      # 1) Tag should be branch.<patch_version>
      # 2) Tag should match the crate version numbers in the manifest files (which get incremented by the next step)
      - name: Confirm tag, branch, and cargo version numbers
        run: scripts/confirm-cargo-version-numbers-before-bump.sh  ${{ github.event.release.target_commitish }} ${{ github.event.release.tag_name }}

      - name: Update Patch Version Numbers
        run: |
          OUTPUT=$(scripts/increment-cargo-version.sh patch)
          SOLANA_NEW_VERSION=$(sed -E 's/.* -> //' <<< $OUTPUT)
          echo "SOLANA_NEW_VERSION=$SOLANA_NEW_VERSION"
          echo "SOLANA_NEW_VERSION=$SOLANA_NEW_VERSION" >> $GITHUB_ENV

      - name: Cargo Tree
        run: ./scripts/cargo-for-all-lock-files.sh tree

      - name: Create Pull Request
        uses: peter-evans/create-pull-request@v4
        with:
          commit-message: Bump Version to ${{ env.SOLANA_NEW_VERSION }}
          title: Bump Version to ${{ env.SOLANA_NEW_VERSION }}
          body: PR opened by Github Action
          branch: update-version-${{ env.SOLANA_NEW_VERSION }}
          base: ${{ github.event.release.target_commitish }}
          labels: automerge
