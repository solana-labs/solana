#!/bin/bash
set -e
validator_type=""

while getopts "t:" opt; do
  case "$opt" in
    t) validator_type="$OPTARG";;
    \?) echo "Usage: $0 -t <validator_type>"; exit 1;;
  esac
done

# Check if validator_type is empty
if [ -z "$validator_type" ]; then
  echo "Validator type is required. Usage: $0 -t <validator_type>"
  exit 1
fi

# Define an array of secret files to process
SECRET_FILES=(
    "/home/solana/${validator_type}-accounts/identity.base64"
    "/home/solana/${validator_type}-accounts/vote.base64"
    "/home/solana/${validator_type}-accounts/stake.base64"
)

# Define an array of decoded file paths
DECODED_FILES=(
    "/home/solana/identity.json"
    "/home/solana/vote.json"
    "/home/solana/stake.json"
)

if [ "$validator_type" == "bootstrap" ]; then
    echo "Validator type is equal to 'bootstrap'"
    SECRET_FILES+=(
      "/home/solana/${validator_type}-accounts/faucet.base64"
    )
    DECODED_FILES+=(
      "/home/solana/faucet.json"
    )
fi

# Loop through the secret files
for i in "${!SECRET_FILES[@]}"; do
    SECRET_FILE="${SECRET_FILES[i]}"
    DECODED_FILE="${DECODED_FILES[i]}"

    # Check if the secret file exists
    if [ -f "$SECRET_FILE" ]; then
        echo "Secret file found at $SECRET_FILE"

        # Read and decode the base64-encoded secret
        SECRET_CONTENT=$(base64 -d < "$SECRET_FILE")

        # Save the decoded secret content to a file
        echo "$SECRET_CONTENT" > "$DECODED_FILE"
        echo "Decoded secret content saved to $DECODED_FILE"
    else
        echo "Secret file not found at $SECRET_FILE"
    fi
done

mkdir -p /home/solana/logs