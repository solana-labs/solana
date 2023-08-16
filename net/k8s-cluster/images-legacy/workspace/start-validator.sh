#!/usr/bin/env bash

./prepare-new-validator-keys.sh

./validator.sh

while true; do
	sleep 100
done
