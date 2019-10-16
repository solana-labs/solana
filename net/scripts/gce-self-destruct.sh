#!/usr/bin/env bash

gce_metadata_req() {
  declare endpoint url
  endpoint="$1"
  url="http://metadata.google.internal/computeMetadata/v1/$endpoint"
  curl -sf -H Metadata-Flavor:Google "$url"
}

gce_self_destruct_setup() {
  declare timeout zone gcloudBase cmd
  timeout="$1"
  zone=$(gce_metadata_req "instance/zone")
  zone=$(basename "$zone")
  gcloudBase=$(dirname $(command -v gcloud))  # XXX: gcloud is installed in /snap/bin,
  gcloudBase=${gcloudBase:-/snap/bin}         #   but /snap/bin isn't in root's PATH...
  cmd="$gcloudBase/gcloud compute instances delete $(hostname) --zone $zone"
  at Now + $timeout Minutes <<<"$cmd"
}
