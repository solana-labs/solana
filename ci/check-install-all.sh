source scripts/spl-token-cli-version.sh
if [[ -z $splTokenCliVersion ]]; then
    echo "On the stable channel, splTokenCliVersion must be set in scripts/spl-token-cli-version.sh"
    exit 1
fi
