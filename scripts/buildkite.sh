#!/bin/bash
export LD_LIBRARY_PATH=/usr/local/cuda/lib64
source $HOME/.cargo/env
export PATH=$PATH:/usr/local/cuda/bin
cp /tmp/libcuda_verify_ed25519.a .
cargo test --features=cuda
