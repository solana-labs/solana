# Create a new stage on AWS
#
# This only needs to be done once per stage
#
# Usage: create-stack.sh dev|preprod|prod
#
# Requirements:
#
# An existing Cloudfront Access Identity (Cloudformation cannot create these)
# An SSL Certificate registered on AWS Certificate Manager
# A hosted zone registered on AWS Route53

set -e
set -u

STAGE=$1

# the ARN of the *.identity.com certificate in prod
PROD_ACCOUNT_CERTIFICATE_ARN="arn:aws:acm:us-east-1:883607224354:certificate/f566be3d-1ced-4120-af39-225a94b8d1b6"
# the ARN of the *.identity.com certificate in dev
DEV_ACCOUNT_CERTIFICATE_ARN="arn:aws:acm:us-east-1:249634870252:certificate/???"

[[ $STAGE = "prod" ]] && ACCESS_IDENTITY="E2183FITBI7GYR" || ACCESS_IDENTITY="EYEQG9MNKSMO6"
[[ $STAGE = "prod" ]] && HOSTED_ZONE_ID="Z32EVO4M2RQREC" || HOSTED_ZONE_ID=""
[[ $STAGE = "prod" ]] && CERTIFICATE_ARN="${PROD_ACCOUNT_CERTIFICATE_ARN}" || CERTIFICATE_ARN="${DEV_ACCOUNT_CERTIFICATE_ARN}"

[[ $STAGE = "prod" ]] && DOMAIN_PREFIX="explorer" || DOMAIN_PREFIX="explorer-${STAGE}"
PROJECT="gateway"
WEBSITE_NAME="${DOMAIN_PREFIX}.identity.com"

aws cloudformation create-stack --stack-name ${DOMAIN_PREFIX}-identity-com \
  --template-body file:///$PWD/secure-cloudfront-s3-website.yml \
  --parameters \
  ParameterKey=CloudFrontOriginPath,ParameterValue=/"${STAGE}" \
  ParameterKey=S3BucketName,ParameterValue=${WEBSITE_NAME} \
  ParameterKey=WebsiteAddress,ParameterValue=${WEBSITE_NAME} \
  ParameterKey=TlsCertificateArn,ParameterValue="${CERTIFICATE_ARN}" \
  ParameterKey=CloudFrontAccessIdentity,ParameterValue="${ACCESS_IDENTITY}" \
  ParameterKey=HostedZoneID,ParameterValue="${HOSTED_ZONE_ID}" \
  --tags \
  Key=project,Value=${PROJECT} \
  Key=owner,Value="Daniel Kelleher" \
  Key=contact,Value=daniel@civic.com
