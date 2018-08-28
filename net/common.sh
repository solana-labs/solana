# |source| this file
#
# Common utilities shared by other scripts in this directory
#
# The following directive disable complaints about unused variables in this
# file:
# shellcheck disable=2034
#

netConfigDir="$(dirname "${BASH_SOURCE[0]}")"/config
netLogDir="$(dirname "${BASH_SOURCE[0]}")"/log
mkdir -p "$netConfigDir" "$netLogDir"

configFile="$netConfigDir/config.sh"

clientIpList=()
leaderIp=
sshPrivateKey=
sshUsername=
sshOptions=()
validatorIpList=()

loadConfigFile() {
  [[ -r $configFile ]] || usage "Config file unreadable: $configFile"

  # shellcheck source=/dev/null
  source "$configFile"
  [[ -n "$leaderIp" ]] || usage "Config file invalid, leaderIp unspecified: $configFile"
  [[ ${#validatorIpList[@]} -gt 0 ]] || usage "Config file invalid, validatorIpList unspecified: $configFile"
  [[ -n $sshUsername ]] || usage "Config file invalid, sshUsername unspecified: $configFile"
  [[ -n $sshPrivateKey ]] || usage "Config file invalid, sshPrivateKey unspecified: $configFile"

  sshOptions=(
    -o "BatchMode=yes"
    -o "StrictHostKeyChecking=no"
    -o "UserKnownHostsFile=/dev/null"
    -o "User=$sshUsername"
    -o "IdentityFile=$sshPrivateKey"
    -o "LogLevel=ERROR"
  )
}
