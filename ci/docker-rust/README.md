Docker image containing rust and some preinstalled packages used in CI.

This image manually maintained:
1. Edit `Dockerfile` to match the desired rust version
1. Run `docker login` to enable pushing images to Docker Hub, if you're authorized.
1. Run `./build.sh` to publish the new image, if you are a member of the [Solana
   Labs](https://hub.docker.com/u/solanalabs/) Docker Hub organization.

