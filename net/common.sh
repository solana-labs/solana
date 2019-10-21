# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

netDir=$(
  cd "$(dirname "${BASH_SOURCE[0]}")" || exit
  echo "$PWD"
)
netConfigDir="$netDir"/config
netLogDir="$netDir"/log
mkdir -p "$netConfigDir" "$netLogDir"

SOLANA_ROOT="$netDir"/..
# shellcheck source=scripts/configure-metrics.sh
source "$SOLANA_ROOT"/scripts/configure-metrics.sh

configFile="$netConfigDir/config"
geoipConfigFile="$netConfigDir/geoip.yml"

entrypointIp=
publicNetwork=
netBasename=
sshPrivateKey=
letsEncryptDomainName=
externalNodeSshKey=
sshOptions=()
validatorIpList=()
validatorIpListPrivate=()
validatorIpListZone=()
clientIpList=()
clientIpListPrivate=()
clientIpListZone=()
blockstreamerIpList=()
blockstreamerIpListPrivate=()
blockstreamerIpListZone=()
archiverIpList=()
archiverIpListPrivate=()
archiverIpListZone=()

buildSshOptions() {
  sshOptions=(
    -o "ConnectTimeout=20"
    -o "BatchMode=yes"
    -o "StrictHostKeyChecking=no"
    -o "UserKnownHostsFile=/dev/null"
    -o "User=solana"
    -o "IdentityFile=$sshPrivateKey"
    -o "LogLevel=ERROR"
  )

  [[ -z $externalNodeSshKey ]] || sshOptions+=(-o "IdentityFile=$externalNodeSshKey")
}

loadConfigFile() {
  [[ -r $configFile ]] || usage "Config file unreadable: $configFile"

  # shellcheck source=/dev/null
  source "$configFile"
  [[ -n "$publicNetwork" ]] || usage "Config file invalid, publicNetwork unspecified: $configFile"
  [[ -n "$netBasename" ]] || usage "Config file invalid, netBasename unspecified: $configFile"
  [[ -n $sshPrivateKey ]] || usage "Config file invalid, sshPrivateKey unspecified: $configFile"
  [[ ${#validatorIpList[@]} -gt 0 ]] || usage "Config file invalid, validatorIpList unspecified: $configFile"
  [[ ${#validatorIpListPrivate[@]} -gt 0 ]] || usage "Config file invalid, validatorIpListPrivate unspecified: $configFile"
  [[ ${#validatorIpList[@]} -eq ${#validatorIpListPrivate[@]} ]] || usage "Config file invalid, validatorIpList/validatorIpListPrivate length mismatch: $configFile"

  if $publicNetwork; then
    entrypointIp=${validatorIpList[0]}
  else
    entrypointIp=${validatorIpListPrivate[0]}
  fi

  buildSshOptions
  configureMetrics
}

# https://gist.github.com/cdown/1163649
urlencode() {
  declare s="$1"
  declare l=$((${#s} - 1))
  for i in $(seq 0 $l); do
    declare c="${s:$i:1}"
    case $c in
      [a-zA-Z0-9.~_-])
        echo -n "$c"
        ;;
      *)
        printf '%%%02X' "'$c"
        ;;
    esac
  done
}

SOLANA_CONFIG_DIR=$SOLANA_ROOT/config
# Clear the current cluster configuration
clear_config_dir() {
  declare config_dir="$1"

  setup_secondary_mount

  (
    set -x
    rm -rf "${config_dir:?}/" # <-- $i might be a symlink, rm the other side of it first
    rm -rf "$config_dir"
    mkdir -p "$config_dir"
  )
}

SECONDARY_DISK_MOUNT_POINT=/mnt/extra-disk
setup_secondary_mount() {
  # If there is a secondary disk, symlink the config/ dir there
  (
    set -x
    if [[ -d $SECONDARY_DISK_MOUNT_POINT ]] && \
      [[ -w $SECONDARY_DISK_MOUNT_POINT ]]; then
      mkdir -p $SECONDARY_DISK_MOUNT_POINT/config
      rm -rf "$SOLANA_CONFIG_DIR"
      ln -sfT $SECONDARY_DISK_MOUNT_POINT/config "$SOLANA_CONFIG_DIR"
    fi
  )
}

