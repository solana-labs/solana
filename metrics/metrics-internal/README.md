![image](https://user-images.githubusercontent.com/110216567/182764431-504557e4-92ac-41ff-82a5-b87c88c19c1d.png)


Services :
1. Influxdb
2. Chronograf (on port 8888)
3. Chronograf_8889 (on port 8889)
4. Grafana

To install all the services on the metrics-internal server you need to run the `start.sh` script.

Install the Buildkite-agent to run the `status.sh` script to periodically check for the status of the containers.

If any of the containers is not in running state or in exited state then it will try to redeploy the container, if it fails to do so an alert will be triggered to Discord and PagerDuty.

**Note:** If you deleted or removed any of containers manually you need to run the `start.sh` script.
