#!/usr/bin/env bash
set -ex

[[ $(uname) = Linux ]] || exit 1
[[ $USER = root ]] || exit 1

apt-get update
add-apt-repository --yes ppa:certbot/certbot
apt-get --assume-yes install certbot

cat > /certbot-restore.sh <<'EOF'
#!/usr/bin/env bash
set -e

domain=$1
email=$2

if [[ $USER != root ]]; then
  echo "Run as root"
  exit 1
fi

if [[ -f /.cert.pem ]]; then
  echo "Certificate already initialized"
  exit 0
fi

set -x
if [[ -r letsencrypt.tgz ]]; then
  tar -C / -zxf letsencrypt.tgz
fi

cd /
rm -f letsencrypt.tgz

maybeDryRun=
# Uncomment during testing to avoid hitting LetsEncrypt API limits while iterating
#maybeDryRun="--dry-run"

certbot certonly --standalone -d "$domain" --email "$email" --agree-tos -n $maybeDryRun

tar zcf letsencrypt.tgz /etc/letsencrypt
ls -l letsencrypt.tgz

# Copy certificates to / for easy access without knowing the value of "$domain"
rm -f /.key.pem /.cert.pem
cp /etc/letsencrypt/live/$domain/privkey.pem /.key.pem
cp /etc/letsencrypt/live/$domain/cert.pem /.cert.pem

EOF

chmod +x /certbot-restore.sh
