# |source| this file
#
# Adjusts system settings for optimal validator performance
#

sysctl_write() {
  declare name=$1
  declare new_value=$2

  # Test the existence of the sysctl before trying to set it
  sysctl "$name" 2>/dev/null 1>/dev/null || return 0

  declare current_value
  current_value=$(sysctl -n "$name")
  [[ $current_value != "$new_value" ]] || return 0

  declare cmd="sysctl -w $name=$new_value"
  if [[ -n $SUDO_OK ]]; then
    cmd="sudo $cmd"
  fi

  echo "$ $cmd"
  $cmd || true

  # Some versions of sysctl exit with 0 on permission denied errors
  current_value=$(sysctl -n "$name")
  if [[ $current_value != "$new_value" ]]; then
    echo "==> Failed to set $name.  Try running: \"SUDO_OK=1 source ${BASH_SOURCE[0]}\""
  fi
}

case $(uname) in
Linux)
  # Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
  sysctl_write net.core.rmem_max 134217728
  sysctl_write net.core.rmem_default 134217728
  sysctl_write net.core.wmem_max 134217728
  sysctl_write net.core.wmem_default 134217728
  ;;
*)
  ;;
esac

