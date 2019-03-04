#!/usr/bin/env bash
#
# Reference: https://github.com/nodesource/distributions/blob/master/README.md#deb
#
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

# Install node/npm
curl -sL https://deb.nodesource.com/setup_10.x | bash -
apt-get install -y nodejs
node --version
npm --version

# Install yarn
curl -sL https://dl.yarnpkg.com/debian/pubkey.gpg | apt-key add -
echo "deb https://dl.yarnpkg.com/debian/ stable main" | tee /etc/apt/sources.list.d/yarn.list
apt-get update -qq
apt-get install -y yarn
yarn --version
