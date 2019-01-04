#!/bin/bash

# these keys don't work - get demo keys at https://pubnub.com
PN_PUB_KEY=pub-c-00000000-0000-0000-0000-000000000000
PN_SUB_KEY=sub-c-00000000-0000-0000-0000-000000000000
NODE_INFO=$(curl -q -s ifconfig.co/json)

export PN_PUB_KEY
export PN_SUB_KEY
export NODE_INFO

yarn run babel-node --presets env index.js
