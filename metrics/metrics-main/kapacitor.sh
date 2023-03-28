#if there is an existing service it needs to be deleted otherwise it will get an error
sudo docker kill kapacitor
sudo docker rm -f kapacitor


#running influx kapacitor service
sudo docker run \
  --detach \
  --name=kapacitor \
  --publish 9092:9092 \
  --volume "$PWD"/kapacitor.conf:/etc/kapacitor/kapacitor.conf \
  --volume /var/lib/kapacitor:/var/lib/kapacitor \
  --user "$(id -u):$(id -g)" \
  --log-opt max-size=1g \
  --log-opt max-file=5  \
  kapacitor:1.6.5
