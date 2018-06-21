FROM snapcraft/xenial-amd64

# Update snapcraft to latest version
RUN apt-get update -qq \
 && apt-get install -y snapcraft \
 && rm -rf /var/lib/apt/lists/* \
 && snapcraft --version
