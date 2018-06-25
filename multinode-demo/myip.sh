#!/bin/bash

function myip()
{
  # shellcheck disable=SC2207
  declare ipaddrs=(
    # query interwebs
    $(curl -s ifconfig.co)
    # machine's interfaces
    $(ifconfig |
          awk '/inet addr:/ {gsub("addr:","",$2); print $2; next}
               /inet6 addr:/ {gsub("/.*", "", $3); print $3; next}
               /inet(6)? / {print $2}'
    )
  )

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
