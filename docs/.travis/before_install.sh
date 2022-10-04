# |source| this file

curl -sL https://deb.nodesource.com/setup_16.x | sudo -E bash -
sudo apt install -y nodejs

npm install --global docusaurus-init
docusaurus-init

npm install --global vercel
