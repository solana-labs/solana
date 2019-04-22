# Metrics

## Testnet Grafana Dashboard

There are three versions of the testnet dashboard, corresponding to the three
release channels:
* https://metrics.solana.com:3000/d/testnet-edge/testnet-monitor-edge
* https://metrics.solana.com:3000/d/testnet-beta/testnet-monitor-beta
* https://metrics.solana.com:3000/d/testnet/testnet-monitor

The dashboard for each channel is defined from the
`metrics/testnet-monitor.json` source file in the git branch associated with
that channel, and deployed by automation running `ci/publish-metrics-dashboard.sh`.

A deploy can be triggered at any time via the `New Build` button of
https://buildkite.com/solana-labs/publish-metrics-dashboard.

### Modifying a Dashboard

Dashboard updates are accomplished by modifying `metrics/testnet-monitor.json`,
**manual edits made directly in Grafana will be overwritten**.

* Check out metrics to add at https://metrics.solana.com:8888/ in the data explorer.
* When editing a query for a dashboard graph, use the "Toggle Edit Mode" selection
  behind the hamburger button to use raw SQL and copy the query into the text field.
  You may have to fixup the query with the dashboard variables like $testnet or $timeFilter,
  check other functioning fields in the dashboard for examples.

1. Open the desired dashboard in Grafana
2. Create a development copy of the dashboard by selecting `Save As..` in the
   `Settings` menu for the dashboard
3. Edit dashboard as desired
4. Extract the JSON Model by selecting `JSON Model` in the `Settings` menu.  Copy the JSON to the clipboard
    and paste into `metrics/testnet-monitor.json`
5. Delete your development dashboard: `Settings` => `Delete`

### Deploying a Dashboard Manually

If you need to immediately deploy a dashboard using the contents of
`metrics/testnet-monitor.json` in your local workspace,
```
$ export GRAFANA_API_TOKEN="an API key from https://metrics.solana.com:3000/org/apikeys"
$ metrics/publish-metrics-dashboard.sh (edge|beta|stable)
```
Note that automation will eventually overwrite your manual deploy.
