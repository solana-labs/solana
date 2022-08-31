#!/usr/bin/env bash

cargo build --release
NDEBUG=1 ./multinode-demo/setup.sh

base_log_dir="bench-tps-logs"
mkdir ${base_log_dir}

NDEBUG=1 ./multinode-demo/faucet.sh &> ${base_log_dir}/faucet.log &
faucet_job=$!
echo "Faucet job: ${faucet_job} with log at ${base_log_dir}/faucet.log"
sleep 5

NDEBUG=1 ./multinode-demo/bootstrap-validator.sh &> ${base_log_dir}/bootstrap-validator.log &
bootstrap_validator_job=$!
echo "Bootstrap validator job: ${bootstrap_validator_job} with log at ${base_log_dir}/bootstrap-validator.log"
sleep 5

# potential race condition with the validator starting up. Added a long 5s wait after starting the bootstrap-validator
NDEBUG=1 ./multinode-demo/bench-tps.sh | tee ${base_log_dir}/bench-tps.log

kill -9 ${faucet_job}
kill -9 ${bootstrap_validator_job}