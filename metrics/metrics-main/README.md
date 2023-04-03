![image](https://user-images.githubusercontent.com/110216567/184346286-94e0b45f-19e9-4fc9-a1a3-2e50c6f12bf8.png)

Services:
1. Prometheus
2. AlertManager
3. Chronograf2 (on port 8888)
4. Chronograf_8889 (on port 8889)
5. Grafana (on port 3000)
6. Grafana2 (on port 3001)
7. Kapacitor

To install all the services on the metrics-internal server, you need to run the ./start.sh script.

Install the Buildkite-agent to run the pipeline to get the status of the container.

If any of the containers is not in running state or in exited state then it will redeploy the container as per the specific container status.

**Note:** If you delete or remove the container manually then you can also run the script to redeploy it  again.
