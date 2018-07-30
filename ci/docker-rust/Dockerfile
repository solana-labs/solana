FROM rust

RUN apt update && \
    apt-get install apt-transport-https && \
    echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list && \
    apt-key adv --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 && \
    apt update && \
    apt install -y \
      buildkite-agent \
      rsync \
      sudo \
      && \
    rm -rf /var/lib/apt/lists/*
