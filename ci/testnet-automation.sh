#!/bin/bash -e

echo --- downloading snap from build artifacts
buildkite-agent artifact download "solana_*.snap" .

# shellcheck disable=SC1091
source ci/upload_ci_artifact.sh

launchTestnet() {
  echo --- setup "$1" node test
  net/gce.sh create -n "$1" -c 2 -G "n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" -p testnet-automation -z us-west1-b

  echo --- configure database
  net/init-metrics.sh -e

  echo --- start "$1" node test
  net/net.sh start -o noValidatorSanity -S solana_*.snap

  echo --- wait 600 seconds to complete test
  sleep 600

  MEAN_TPS_QUERY="SELECT round(mean(\"sum_count\")) from (SELECT sum(\"count\") AS \"sum_count\" FROM \"testnet-automation\".\"autogen\".\"counter-banking_stage-process_transactions\" WHERE time > now() - 10m GROUP BY time(1s))"
  MAX_TPS_QUERY="SELECT max(\"sum_count\") from (SELECT sum(\"count\") AS \"sum_count\" FROM \"testnet-automation\".\"autogen\".\"counter-banking_stage-process_transactions\" WHERE time > now() - 10m GROUP BY time(1s))"
  MEAN_FINALITY_QUERY="SELECT mean(\"duration_ms\") FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 10m"
  MAX_FINALITY_QUERY="SELECT max(\"duration_ms\") FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 10m"
  FINALITY_99TH_QUERY="SELECT percentile(\"duration_ms\", 99) FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 10m"

  curl -G "$METRICS_URL" --data-urlencode "db=$INFLUX_DATABASE" --data-urlencode "q=$MEAN_TPS_QUERY;$MAX_TPS_QUERY;$MEAN_FINALITY_QUERY;$MAX_FINALITY_QUERY;$FINALITY_99TH_QUERY" >>TPS"$1".log

  upload_ci_artifact TPS"$1".log
}

launchTestnet 10
launchTestnet 25
launchTestnet 50
launchTestnet 100

echo --- delete testnet
net/gce.sh delete -p testnet-automation
