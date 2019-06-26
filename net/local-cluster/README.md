#Bring up Instructions for Local Cluster Machines

## Setting up new machines
### Hardware
Build the box.  Include hardware list, assembly instructions and any particular construction needs
ASRock mobo does not have built in graphics support.  Need to plug in a GPU to get a direct monitor connection for initial bringup.  (Can we find a way around this?)
### BIOS
Needed BIOS settings:
 * Secure erase any new SSDs
 * Auto-boot on power up due to remote/external power cycle
 * Boot device is correct SSD
 
### OS
Configuring the OS  Ubuntu 18.04 LTS Server
* Machine needs to be plugged in to Ethernet for OS to install on initial boot.
* USB stick with 18.04 LTS Server image
* Set up passwordless sudo access from pubkeys

### Set up Wi-Fi
https://www.linuxbabe.com/command-line/ubuntu-server-16-04-wifi-wpa-supplicant

### SSH Port forwarding
Try not to break Greg's home network again.  Use a random SSH port for forwarding

### Install Solana tools and CUDA
