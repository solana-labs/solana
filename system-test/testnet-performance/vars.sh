#!/usr/bin/env bash

#set -x
SOLANA_METRICS_PARTIAL_CONFIG="u=scratch_writer,p=topsecret"
CHANNEL="edge"
UPLOAD_RESULTS_TO_SLACK="false"
CLOUD_PROVIDER="ec2"
TESTNET_TAG="aws-perf-cpu-only"
RAMP_UP_TIME=0
TEST_DURATION_SECONDS=300
NUMBER_OF_VALIDATOR_NODES=1
ENABLE_GPU="false"
# Closest to GCE default, 16 vCPU and 64 GB Mem
VALIDATOR_NODE_MACHINE_TYPE="m5.4xlarge"
NUMBER_OF_CLIENT_NODES="1"
CLIENT_OPTIONS="bench-tps=1=--tx_count 15000 --thread-batch-sleep-ms 250"
TESTNET_ZONES="us-west-1a"
