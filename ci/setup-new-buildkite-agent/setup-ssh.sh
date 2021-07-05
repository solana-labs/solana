#!/usr/bin/env bash

HERE="$(dirname "$0")"

# shellcheck source=ci/setup-new-buildkite-agent/utils.sh
source "$HERE"/utils.sh

ensure_env || exit 1
# This is a last ditch effort to prevent the caller from locking themselves
# out of the machine. Exiting here will likely leave the system in some
# half-configured state. To prevent this, duplicate the next line at the top
# of the entrypoint script.
check_ssh_authorized_keys || exit 1

set -xe
# Setup sshd
sed -i 's/^PasswordAuthentication yes//' /etc/ssh/sshd_config
sed -i 's/^#\(PasswordAuthentication\) yes/\1 no/' /etc/ssh/sshd_config
sed -i 's/^#\(PermitRootLogin\) .*/\1 no/' /etc/ssh/sshd_config
systemctl restart sshd
