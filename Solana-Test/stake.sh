#!/bin/bash
# I got data from this command --> solana -um stakes --url mainnet-beta -v > /home/solana/stake.txt and I fo>
printf "%s: %s\n" "Solana Lab"
printf "%s: %s\n" "*********************************************************************"

printf "%s: %s\n" "Active Stake Number : " "$(grep -wc "Active Stake" /home/solana/stake.txt)"
printf "%s: %s\n" "Deactive Stake Number : " "$(grep -wc "Stake account is undelegated" /home/solana/stake.t>

printf "%s: %s\n" "*********************************************************************"

printf "%s: %s\n" "Okcan Yasin"
