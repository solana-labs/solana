echo --- downloading snap from build artifacts
buildkite-agent artifact download "solana_*.snap" .

echo --- setup 10 node test
net/gce.sh create -n 10 -c 2 -G "n1-standard-16 --accelerator count=2,type=nvidia-tesla-v100" -p testnet-automation -z us-west1-b

echo --- configure database
net/init-metrics.sh -e

echo --- start 10 node test
net/net.sh start -o noValidatorSanity -S solana_*.snap

sleep 600

MEAN_TPS_QUERY="SELECT round(mean(\"sum_count\")) from (SELECT sum(\"count\") AS \"sum_count\" FROM \"testnet-automation\".\"autogen\".\"counter-banking_stage-process_transactions\" WHERE time > now() - 1h GROUP BY time(1s))"
MAX_TPS_QUERY="SELECT max(\"sum_count\") from (SELECT sum(\"count\") AS \"sum_count\" FROM \"testnet-automation\".\"autogen\".\"counter-banking_stage-process_transactions\" WHERE time > now() - 1h GROUP BY time(1s))"
MEAN_FINALITY_QUERY="SELECT mean(\"duration_ms\") FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 1h"
MAX_FINALITY_QUERY="SELECT max(\"duration_ms\") FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 1h"
FINALITY_99TH_QUERY="SELECT percentile(\"duration_ms\", 99) FROM \"testnet-automation\".\"autogen\".\"leader-finality\" WHERE time > now() - 1h"

curl -G "$METRICS_URL" --data-urlencode "db=$INFLUX_DATABASE" --data-urlencode "q=$MEAN_TPS_QUERY" >> TPS.log
#curl -G \"$METRICS_URL\" --data-urlencode \"db=$INFLUX_DATABASE\" --data-urlencode \"q=$MAX_TPS_QUERY\"
#curl -G \"$METRICS_URL\" --data-urlencode \"db=$INFLUX_DATABASE\" --data-urlencode \"q=$MEAN_FINALITY_QUERY\"
#curl -G \"$METRICS_URL\" --data-urlencode \"db=$INFLUX_DATABASE\" --data-urlencode \"q=$MAX_FINALITY_QUERY\"
#curl -G \"$METRICS_URL\" --data-urlencode \"db=$INFLUX_DATABASE\" --data-urlencode \"q=$FINALITY_99TH_QUERY\"

source upload_ci_artifacts.sh
upload_ci_artifacts TPS.log

echo --- delete testnet
net/gce.sh delete -p testnet-automation