# populate this on the stable branch
splTokenCliVersion=3.4.1

maybeSplTokenCliVersionArg=
if [[ -n "$splTokenCliVersion" ]]; then
    # shellcheck disable=SC2034
    maybeSplTokenCliVersionArg="--version $splTokenCliVersion"
fi
