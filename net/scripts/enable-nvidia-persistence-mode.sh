#!/usr/bin/env bash

echo "--- Enabling nvidia persistence mode"
if ! nvidia-smi -pm ENABLED; then
  echo "^^^ +++"
fi
