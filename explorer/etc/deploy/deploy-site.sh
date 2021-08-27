#!/usr/bin/env bash
set -e
set -u

if [ "${STAGE}" == "prod" ]; then
  DISTRIBUTION=EZ0FP8B01RWC9
  BUCKET=explorer.identity.com
elif [ ${STAGE} == "preprod" ]; then
  DISTRIBUTION=???
  BUCKET=explorer-preprod.identity.com
elif [ ${STAGE} == "dev" ]; then
  DISTRIBUTION=???
  BUCKET=explorer-dev.identity.com
fi

npx deploy-aws-s3-cloudfront --non-interactive --react --bucket ${BUCKET} --destination ${STAGE} --distribution ${DISTRIBUTION}

