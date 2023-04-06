![image](https://user-images.githubusercontent.com/110216567/182764431-504557e4-92ac-41ff-82a5-b87c88c19c1d.png)


Services :
1. Influxdb
2. Chronograf (on port 8888)
3. Chronograf_8889 (on port 8889)
4. Grafana

To install all the services on metrics-internal server, you need to run the ./start.sh script

Install the Buildkite-agent to run the pipeline to get the status of the container.

If any of the containers is not in running state or in exited state then it will redeploy the container as per the specific container status.

**Note:** If you delete or remove the container manually then you can also run the specific script to redeploy it again.
