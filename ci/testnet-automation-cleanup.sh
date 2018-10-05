#!/bin/bash -e

echo --- find testnet configuration
net/gce.sh config -p testnet-automation

echo --- delete testnet
net/gce.sh delete -p testnet-automation
