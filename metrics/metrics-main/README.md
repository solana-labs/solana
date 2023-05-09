![image](https://user-images.githubusercontent.com/110216567/184346286-94e0b45f-19e9-4fc9-a1a3-2e50c6f12bf8.png)

Services:
1. Prometheus
2. AlertManager
3. Chronograf (on port 8888)
4. Chronograf_8889 (on port 8889)
5. Grafana (on port 3000)
6. AlertManager_Discord
7. Kapacitor

To install all the services on the metrics-main server you need to run the `start.sh` script.

Install the Buildkite-agent to run the `status.sh` script to periodically check for the status of the containers.

If any of the containers is not in running state or in exited state then it will try to redeploy the container, if it fails to do so an alert will be triggered to Discord and PagerDuty.

**Note:** If you deleted or removed any of containers manually you need to run the `start.sh` script.
