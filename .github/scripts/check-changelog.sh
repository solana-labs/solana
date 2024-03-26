#!/bin/bash
set -uo pipefail

CHANGELOG_FILE="CHANGELOG.md"
echo "Checking: git diff --exit-code origin/${BASE_REF} -- ${CHANGELOG_FILE}"

if git diff --exit-code "origin/${BASE_REF}" -- "${CHANGELOG_FILE}"; then
    >&2 echo "Error: this pull request requires an entry in $CHANGELOG_FILE, but no entry was found"
    exit 1
fi
