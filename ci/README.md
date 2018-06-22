
Our CI infrastructure is built around [BuildKite](https://buildkite.com) with some
additional GitHub integration provided by https://github.com/mvines/ci-gate

## Buildkite AWS CloudFormation Setup

We use AWS CloudFormation to scale machines up and down based on the current CI
load.  If no machine is currently running it can take up to 60 seconds to spin
up a new instance, please remain calm during this time.

### Agent Queues

We define two [Agent Queues](https://buildkite.com/docs/agent/v3/queues):
`queue=default` and `queue=cuda`.  The `default` queue should be favored and
runs on lower-cost CPU instances.  The `cuda` queue is only necessary for
running **tests** that depend on GPU (via CUDA) access -- CUDA builds may still
be run on the `default` queue, and the [buildkite artifact
system](https://buildkite.com/docs/builds/artifacts) used to transfer build
products over to a GPU instance for testing.

### AMI
We use a custom AWS AMI built via https://github.com/solana-labs/elastic-ci-stack-for-aws/tree/solana/cuda.

Use the following process to update this AMI as dependencies change:
```bash
$ export AWS_ACCESS_KEY_ID=my_access_key
$ export AWS_SECRET_ACCESS_KEY=my_secret_access_key
$ git clone https://github.com/solana-labs/elastic-ci-stack-for-aws.git -b solana/cuda
$ cd elastic-ci-stack-for-aws/
$ make build
$ make build-ami
```

Watch for the *"amazon-ebs: AMI:"* log message to extract the name of the new
AMI.  For example:
```
amazon-ebs: AMI: ami-07118545e8b4ce6dc
```
The new AMI should also now be visible in your EC2 Dashboard.  Go to the desired
AWS CloudFormation stack, update the **ImageId** field to the new AMI id, and
*apply* the stack changes.


