#!/usr/bin/env bash
set -euof pipefail

team_to_add_slug="security-incident-response"
github_org="anza-xyz"
github_repo="agave"

# Note: This will get all the GHSAs even if there are more than the per_page value
# from gh api --help
# --paginate    Make additional HTTP requests to fetch all pages of results
ghsa_json=$(gh api \
    -H "Accept: application/vnd.github+json" \
    -H "X-GitHub-Api-Version: 2022-11-28"   \
    /repos/$github_org/$github_repo/security-advisories?per_page=100 --paginate )

# Get a list of GHSAs that don't have the $team_to_add_slug in collaborating_teams
ghsa_without_team=$( jq -r '[ .[] | select(all(.collaborating_teams.[]; .slug != "'"$team_to_add_slug"'")) | .ghsa_id ] | sort | .[] ' <<< "$ghsa_json" )
if [[ -z $ghsa_without_team ]]; then
    echo "All GHSAs already have $team_to_add_slug. Exiting..."
    exit 0
fi

# Iterate through the teams
while IFS= read -r ghsa_id; do
    # PATCH updates the value. If we just set -f "collaborating_teams[]=$team_to_add_slug" it
    # will overwrite any existing collaborating_teams. So we get the list of teams that are already
    # added to this GHSA and format them as parameters for gh api like:
    #    -f collaborating_teams[]=ghsa-testing-1
    original_collaborating_team_slugs=$( jq -r '[ .[] | select(.ghsa_id == "'"$ghsa_id"'") | .collaborating_teams ] | "-f collaborating_teams[]=" + .[][].slug ' <<< "$ghsa_json" )

    # Update the team list
    # shellcheck disable=SC2086
    gh api \
        --method PATCH \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "/repos/$github_org/$github_repo/security-advisories/$ghsa_id" \
        -f "collaborating_teams[]=$team_to_add_slug" $original_collaborating_team_slugs \
        > /dev/null 2>&1
done <<< "$ghsa_without_team"
