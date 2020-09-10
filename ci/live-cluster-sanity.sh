#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."

source ci/_
source ci/rust-version.sh stable
source ci/upload-ci-artifact.sh

escaped_branch=$(echo "$BUILDKITE_BRANCH" | tr -c "[:alnum:]" - | sed -r "s#(^-*|-*head-*|-*$)##g")
instance_prefix="testnet-live-sanity-$escaped_branch"
# ensure to delete leftover cluster
./net/gce.sh delete -p "$instance_prefix" || true
# only bootstrap, no normal validator
./net/gce.sh create -p "$instance_prefix" -n 0
instance_ip=$(./net/gce.sh info | grep bootstrap-validator | awk '{print $3}')

on_trap() {
  if [[ -z $instance_deleted ]]; then
    (
      set +e
      upload-ci-artifact cluster-sanity/testnet-validator.log
      upload-ci-artifact cluster-sanity/mainnet-beta-validator.log
      _ ./net/gce.sh delete -p "$instance_prefix"
    )
  fi
}
trap on_trap INT TERM EXIT

_ cargo +"$rust_stable" build --bins --release
_ ./net/scp.sh ./target/release/solana-validator "$instance_ip":.
echo 500000 | ./net/ssh.sh "$instance_ip" sudo tee /proc/sys/vm/max_map_count > /dev/null

test_with_live_cluster() {
  cluster_label="$1"
  shift

  echo "--- Starting validator $cluster_label"

  rm -rf cluster-sanity
  mkdir cluster-sanity
  ./net/ssh.sh "$instance_ip" rm -rf cluster-sanity
  ./net/ssh.sh "$instance_ip" mkdir cluster-sanity

  validator_log="cluster-sanity/$cluster_label-validator.log"
  (./net/ssh.sh "$instance_ip" -Llocalhost:18899:localhost:18899 ./solana-validator \
    --no-untrusted-rpc \
    --ledger cluster-sanity/ledger \
    --log - \
    --init-complete-file cluster-sanity/init-completed \
    --enable-rpc-exit \
    --private-rpc \
    --rpc-port 18899 \
    --rpc-bind-address localhost \
    --snapshot-interval-slots 0 \
    "$@" ) &> "$validator_log" &
  ssh_pid=$!
  tail -F "$validator_log" > cluster-sanity/log-tail 2> /dev/null &
  tail_pid=$!
  sleep 3

  attempts=100
  while ! ./net/ssh.sh "$instance_ip" test -f cluster-sanity/init-completed &> /dev/null ; do
    attempts=$((attempts - 1))
    if [[ (($attempts == 0)) || ! -d "/proc/$ssh_pid" ]]; then
       set +e
       kill $ssh_pid $tail_pid
       wait $ssh_pid $tail_pid
       echo "Error: validator failed to boot"
       exit 1
    fi

    sleep 3
    echo "##### validator is starting... (until timeout: $attempts) #####"
    if find cluster-sanity/log-tail -not -empty | grep ^ > /dev/null; then
      echo "##### new log:"
      timeout 1 cat cluster-sanity/log-tail | tail -n 3 || true
      truncate --size 0 cluster-sanity/log-tail
      echo
    fi
  done

  snapshot_slot=$(./net/ssh.sh "$instance_ip" ls -t cluster-sanity/ledger/snapshot* |
    head -n 1 |
    grep -o 'snapshot-[0-9]*-' |
    grep -o '[0-9]*'
  )

  echo "--- Monitoring validator $cluster_label"

  attempts=100
  current_root=$snapshot_slot
  goal_root=$((snapshot_slot + 100))
  while [[ $current_root -le $goal_root ]]; do
    attempts=$((attempts - 1))
    if [[ (($attempts == 0)) || ! -d "/proc/$ssh_pid" ]]; then
       set +e
       kill $ssh_pid $tail_pid
       wait $ssh_pid $tail_pid
       echo "Error: validator failed to boot"
       exit 1
    fi

    sleep 3
    current_root=$(./target/release/solana --url http://localhost:18899 slot --commitment root)
    echo "##### validator is running ($current_root/$goal_root)... (until timeout: $attempts) #####"
    if find cluster-sanity/log-tail -not -empty | grep ^ > /dev/null; then
      echo "##### new log:"
      timeout 1 cat cluster-sanity/log-tail | tail -n 3 || true
      truncate --size 0 cluster-sanity/log-tail
      echo
    fi
  done

  _ curl \
    -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1, "method":"validatorExit"}' \
    http://localhost:18899
  sleep 10

  (sleep 3 && kill "$tail_pid") &
  kill_pid=$!
  wait "$ssh_pid" "$tail_pid" "$kill_pid"

  upload-ci-artifact "$validator_log"
}

test_with_live_cluster "mainnet-beta" \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --entrypoint mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --expected-shred-version 64864

test_with_live_cluster "testnet" \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --entrypoint 35.203.170.30:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --expected-shred-version 1579 \

./net/gce.sh delete -p "$instance_prefix" && instance_deleted=yes
