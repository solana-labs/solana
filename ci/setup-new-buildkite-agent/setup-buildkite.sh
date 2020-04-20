#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
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
sudo -u buildkite-agent ssh-keygen -t ecdsa -q -N "" -f ~buildkite-agent/.ssh/id_ecdsa

# Set buildkite-agent user's shell
sudo usermod --shell /bin/bash buildkite-agent

# Install Rust for buildkite-agent
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
sudo -u buildkite-agent HOME=~buildkite-agent sh /tmp/rustup-init.sh -y

# Add to docker and sudoers group
addgroup buildkite-agent docker
addgroup buildkite-agent sudo

# Edit the systemd unit file to include LimitNOFILE
cat > /lib/systemd/system/buildkite-agent.service <<EOF
[Unit]
Description=Buildkite Agent
Documentation=https://buildkite.com/agent
After=syslog.target
After=network.target

[Service]
Type=simple
User=buildkite-agent
Environment=HOME=/var/lib/buildkite-agent
ExecStart=/usr/bin/buildkite-agent start
RestartSec=5
Restart=on-failure
RestartForceExitStatus=SIGPIPE
TimeoutStartSec=10
TimeoutStopSec=0
KillMode=process
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
DefaultInstance=1
EOF
