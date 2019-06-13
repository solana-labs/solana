#!/usr/bin/env bash
set -e

cd "$(dirname "$0")"

CHANNEL=$1
if [[ -z $CHANNEL ]]; then
  echo "usage: $0 [channel]"
  exit 1
fi

case $CHANNEL in
edge)
  DASHBOARD=testnet-monitor-edge
  ;;
beta)
  DASHBOARD=testnet-monitor-beta
  ;;
stable)
  DASHBOARD=testnet-monitor
  ;;
*)
  echo "Error: Invalid CHANNEL=$CHANNEL"
  exit 1
  ;;
esac


if [[ -z $GRAFANA_API_TOKEN ]]; then
  echo Error: GRAFANA_API_TOKEN not defined
  exit 1
fi

DASHBOARD_JSON=./testnet-monitor.json
if [[ ! -r $DASHBOARD_JSON ]]; then
  echo Error: $DASHBOARD_JSON not found
fi

(
  set -x
  ./adjust-dashboard-for-channel.py "$DASHBOARD_JSON" "$CHANNEL" "$DASHBOARD_JSON".out
)

rm -rf venv
python3 -m venv venv
# shellcheck source=/dev/null
source venv/bin/activate

echo --- Fetch/build grafcli
(
  set -x
  git clone git@github.com:mvines/grafcli.git -b experimental-v5 venv/grafcli
  cd venv/grafcli
  python3 setup.py install
)

echo --- Take a backup of existing dashboard if possible
(
  set -x +e
  grafcli export remote/metrics/$DASHBOARD $DASHBOARD_JSON.org
  grafcli rm remote/metrics/$DASHBOARD
  :
)

echo --- Publish $DASHBOARD_JSON to $DASHBOARD
(
  set -x
  grafcli import "$DASHBOARD_JSON".out remote/metrics
)

exit 0
