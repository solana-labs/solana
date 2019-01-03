#!/bin/bash

# these keys don't work - get demo keys at https://pubnub.com
export PN_PUB_KEY=pub-c-00000000-0000-0000-0000-000000000000
export PN_SUB_KEY=sub-c-00000000-0000-0000-0000-000000000000

export NODE_INFO=`curl -q -s ifconfig.co/json`

yarn run babel-node --presets env index.js
