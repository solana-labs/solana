name: error-reporting

on:
  workflow_call:
    secrets:
      WEBHOOK:
        required: true

jobs:
  slack:
    runs-on: ubuntu-20.04
    steps:
      - env:
          COMMIT_MESSAGE: ${{ github.event.head_commit.message }}
          COMMIT_AUTHOR_NAME: ${{ github.event.head_commit.author.name }}
        run: |
          curl -H "Content-Type: application/json" \
            -X POST ${{ secrets.WEBHOOK }} \
            -d '{
              "attachments": [
                {
                  "color": "#AC514C",
                  "text":
                    "*${{ github.repository }} (${{ github.workflow }})*\n'"$COMMIT_MESSAGE"' - _'"$COMMIT_AUTHOR_NAME"'_\n<${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}|View Build>",
                }
              ]
            }'
