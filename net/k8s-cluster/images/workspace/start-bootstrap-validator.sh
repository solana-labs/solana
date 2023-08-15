#!/usr/bin/env bash

./prepare-bootstrap-validator-keys.sh
echo "ran prepare bootstrap"

./generate-genesis.sh
echo "ran generate genesis"

./bootstrap-validator.sh
echo "ran bootstrap"

./faucet.sh
echo "ran faucet"


while true; do
	sleep 100
done
