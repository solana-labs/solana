ip=$(curl ifconfig.me/ip)
nc -vzu $ip 8000-8005
