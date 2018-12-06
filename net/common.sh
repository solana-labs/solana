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

# shellcheck source=scripts/configure-metrics.sh
source "$(dirname "${BASH_SOURCE[0]}")"/../scripts/configure-metrics.sh

configFile="$netConfigDir/config"

entrypointIp=
publicNetwork=
bootstrapFullNodeIp=
netBasename=
sshPrivateKey=
clientIpList=()
sshOptions=()
additionalFullNodeIps=()

buildSshOptions() {
  sshOptions=(
    -o "BatchMode=yes"
    -o "StrictHostKeyChecking=no"
    -o "UserKnownHostsFile=/dev/null"
    -o "User=solana"
    -o "IdentityFile=$sshPrivateKey"
    -o "LogLevel=ERROR"
    -F /dev/null
  )
}

loadConfigFile() {
  [[ -r $configFile ]] || usage "Config file unreadable: $configFile"

  # shellcheck source=/dev/null
  source "$configFile"
  [[ -n "$entrypointIp" ]] || usage "Config file invalid, entrypointIp unspecified: $configFile"
  [[ -n "$publicNetwork" ]] || usage "Config file invalid, publicNetwork unspecified: $configFile"
  [[ -n "$bootstrapFullNodeIp" ]] || usage "Config file invalid, bootstrapFullNodeIp unspecified: $configFile"
  [[ -n "$netBasename" ]] || usage "Config file invalid, netBasename unspecified: $configFile"
  [[ -n $sshPrivateKey ]] || usage "Config file invalid, sshPrivateKey unspecified: $configFile"
  [[ ${#additionalFullNodeIps[@]} -gt 0 ]] || usage "Config file invalid, additionalFullNodeIps unspecified: $configFile"

  buildSshOptions
  configureMetrics
}
