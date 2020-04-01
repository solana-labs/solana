#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=net/datacenter-node-install/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1

set -e

# Install buildkite-agent
echo "deb https://apt.buildkite.com/buildkite-agent stable main" | tee /etc/apt/sources.list.d/buildkite-agent.list
apt-key adv --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198
apt-get update
apt-get install -y buildkite-agent


# Configure the installation
echo "Go to https://buildkite.com/organizations/solana-labs/agents"
echo "Click Reveal Agent Token"
echo "Paste the Agent Token, then press Enter:"

read -r agent_token
sudo sed -i "s/xxx/$agent_token/g" /etc/buildkite-agent/buildkite-agent.cfg

cat > /etc/buildkite-agent/hooks/environment <<EOF
set -e

export BUILDKITE_GIT_CLEAN_FLAGS="-ffdqx"

# Hack for non-docker rust builds
export PATH='$PATH':~buildkite-agent/.cargo/bin

# Add path to snaps
source /etc/profile.d/apps-bin-path.sh

if [[ '$BUILDKITE_BRANCH' =~ pull/* ]]; then
  export BUILDKITE_REFSPEC="+'$BUILDKITE_BRANCH':refs/remotes/origin/'$BUILDKITE_BRANCH'"
fi
EOF

chown buildkite-agent:buildkite-agent /etc/buildkite-agent/hooks/environment

# Create SSH key
sudo -u buildkite-agent mkdir -p ~buildkite-agent/.ssh
sudo -u buildkite-agent ssh-keygen -t ecdsa -q -N "" -f ~buildkite-agent/.ssh/buildkite_ecdsa

# Set buildkite-agent user's shell
sudo usermod --shell /bin/bash buildkite-agent

# Install Rust for buildkite-agent
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
sudo -u buildkite-agent HOME=~buildkite-agent sh /tmp/rustup-init.sh

# Add to docker group
addgroup buildkite-agent docker

echo "************ MANUAL STEP !!! ***********************"
echo "Copy the pubkey contents from ~buildkite-agent/.ssh/buildkite_ecdsa.pub"
echo "Add the pubkey as an authorized SSH key on github:"
cat ~buildkite-agent/.ssh/buildkite_ecdsa.pub
echo "****************************************************"
