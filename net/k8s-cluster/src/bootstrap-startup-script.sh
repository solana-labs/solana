#!/bin/bash

# Define an array of secret files to process
SECRET_FILES=(
    "/home/solana/bootstrap-accounts/identity.base64"
    "/home/solana/bootstrap-accounts/vote.base64"
    "/home/solana/bootstrap-accounts/stake.base64"
)

# Define an array of decoded file paths
DECODED_FILES=(
    "/home/solana/identity.json"
    "/home/solana/vote.json"
    "/home/solana/stake.json"
)

# Loop through the secret files
for i in "${!SECRET_FILES[@]}"; do
    SECRET_FILE="${SECRET_FILES[i]}"
    DECODED_FILE="${DECODED_FILES[i]}"

    # Check if the secret file exists
    if [ -f "$SECRET_FILE" ]; then
        echo "Secret file found at $SECRET_FILE"

        # Read and decode the base64-encoded secret
        SECRET_CONTENT=$(cat "$SECRET_FILE" | base64 -d)

        # Save the decoded secret content to a file
        echo "$SECRET_CONTENT" > "$DECODED_FILE"
        echo "Decoded secret content saved to $DECODED_FILE"
    else
        echo "Secret file not found at $SECRET_FILE"
    fi
done

# Sleep for an hour (3600 seconds)
sleep 3600