#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

add-apt-repository -y ppa:chris-lea/redis-server
apt-get --assume-yes install redis

systemctl enable redis-server.service

REDIS_CONF=/etc/redis/redis.conf

if grep -q "^maxmemory " $REDIS_CONF; then
  echo "setting maxmemory"
  sed -i '/^maxmemory .*/ s//maxmemory 8gb/' $REDIS_CONF
else
  echo "maxmemory not present: appending setting"
  cat << EOF >> $REDIS_CONF

# limit set by solana/net/scripts/install-redis.sh
maxmemory 8gb
EOF

fi

if grep -q "^maxmemory-policy " $REDIS_CONF; then
  echo "setting maxmemory-policy"
  sed -i '/^maxmemory-policy .*/ s//maxmemory-policy allkeys-lru/' $REDIS_CONF
else
  echo "maxmemory-policy not present: appending setting"
  cat << EOF >> $REDIS_CONF
# limit set by solana/net/scripts/install-redis.sh
maxmemory-policy allkeys-lru

EOF
fi

service redis-server restart
