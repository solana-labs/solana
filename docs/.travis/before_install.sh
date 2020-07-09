# |source| this file

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

sudo apt install -y libudev-dev libsystemd-dev

curl -sL https://deb.nodesource.com/setup_12.x | sudo -E bash -
sudo apt install -y nodejs

npm install --global docusaurus-init
docusaurus-init

npm install --global vercel
