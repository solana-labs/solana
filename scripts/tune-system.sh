# |source| this file
#
# Adjusts system settings for optimal fullnode performance
#

# shellcheck source=scripts/ulimit-n.sh
source "$(dirname "${BASH_SOURCE[0]}")"/ulimit-n.sh

# Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
if [[ $(uname) = Linux ]]; then
  (
    set -x +e
    # test the existence of the sysctls before trying to set them
    # go ahead and return true and don't exit if these calls fail
    sysctl net.core.rmem_max 2>/dev/null 1>/dev/null &&
        sudo sysctl -w net.core.rmem_max=161061273 1>/dev/null 2>/dev/null

    sysctl net.core.rmem_default 2>/dev/null 1>/dev/null &&
        sudo sysctl -w net.core.rmem_default=161061273 1>/dev/null 2>/dev/null

    sysctl net.core.wmem_max 2>/dev/null 1>/dev/null &&
        sudo sysctl -w net.core.wmem_max=161061273 1>/dev/null 2>/dev/null

    sysctl net.core.wmem_default 2>/dev/null 1>/dev/null &&
        sudo sysctl -w net.core.wmem_default=161061273 1>/dev/null 2>/dev/null
  ) || true
fi

if [[ $(uname) = Darwin ]]; then
  (
    if [[ $(sysctl net.inet.udp.maxdgram | cut -d\  -f2) != 65535 ]]; then
      echo "Adjusting maxdgram to allow for large UDP packets, see BLOB_SIZE in src/packet.rs:"
      set -x
      sudo sysctl net.inet.udp.maxdgram=65535
    fi
  )

fi


