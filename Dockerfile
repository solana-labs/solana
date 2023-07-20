FROM docker.io/redhat/ubi8

RUN dnf update -y
RUN dnf install -y gcc git clang python3 make curl libudev-devel cmake clang openssl-devel systemd-devel pkg-config zlib-devel llvm  perl-core

COPY cert.pem* /certs/
RUN if [ -e /certs/cert.pem ]; then cp /certs/cert.pem /etc/ssl/cert.pem; fi
RUN if [ -e /certs/cert.pem ]; then cp /certs/cert.pem /etc/pki/tls/cert.pem; fi

WORKDIR /

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rust.sh
RUN chmod a+x rust.sh && ./rust.sh -y

RUN cd /solana; . ~/.cargo/env && ./cargo nightly -Z unstable-options build
