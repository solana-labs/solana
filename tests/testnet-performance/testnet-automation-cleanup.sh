#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/../.."

[[ -n $TESTNET_TAG ]] || TESTNET_TAG=testnet-automation

echo --- find testnet configuration
net/gce.sh config -p $TESTNET_TAG

echo --- delete testnet
net/gce.sh delete -p $TESTNET_TAG
