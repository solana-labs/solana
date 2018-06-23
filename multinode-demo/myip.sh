#!/bin/bash

function myip()
{
  declare ipaddrs=( )

  # query interwebs
  mapfile -t ipaddrs < <(curl -s ifconfig.co)

  # machine's interfaces
  mapfile -t -O "${#ipaddrs[*]}" ipaddrs < \
          <(ifconfig | awk '/inet(6)? (addr:)?/ {print $2}')

  ipaddrs=( "${extips[@]}" "${ipaddrs[@]}" )

  if (( ! ${#ipaddrs[*]} ))
  then
      echo "
myip: error: I'm having trouble determining what our IP address is...
  Are we connected to a network?

"
      return 1
  fi


  declare prompt="
Please choose the IP address you want to advertise to the network:

   0) ${ipaddrs[0]} <====== this one was returned by the interwebs...
"

  for ((i=1; i < ${#ipaddrs[*]}; i++))
  do
    prompt+="   $i) ${ipaddrs[i]}
"
  done

  while read -r -p "${prompt}
please enter a number [0 for default]: " which
  do
    [[ -z ${which} ]] && break;
    [[ ${which} =~ [0-9]+ ]] && (( which < ${#ipaddrs[*]} )) && break;
    echo "Ug. invalid entry \"${which}\"...
"
    sleep 1
  done

  which=${which:-0}

  echo "${ipaddrs[which]}"

}

if [[ ${0} == "${BASH_SOURCE[0]}" ]]
then
    myip "$@"
fi
