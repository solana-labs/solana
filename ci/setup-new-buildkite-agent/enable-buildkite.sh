#!/usr/bin/env bash

sudo systemctl daemon-reload
sudo systemctl enable --now buildkite-agent
