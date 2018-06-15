leader=10.0.5.135
export ips="54.209.198.32 35.174.139.220 54.89.72.66 54.234.242.128 52.23.253.49 52.90.134.252 18.206.88.129 34.238.38.197"

for ff in $ips; do
    echo starting $ff
    ssh $ff -n "export PATH=/home/ubuntu/.cargo:$PATH && cd /home/ubuntu/solana && ./multinode-demo/validator.sh $leader:~/solana" &
done

./multinode-demo/client.sh $leader:~/solana 9
./kill.sh
