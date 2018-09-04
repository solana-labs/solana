Docker image containing rust nightly and some preinstalled crates used in CI.

This image may be manually updated by running `CI=true ./build.sh` if you are a member
of the [Solana Labs](https://hub.docker.com/u/solanalabs/) Docker Hub
organization, but it is also automatically updated periodically by
[this automation](https://buildkite.com/solana-labs/solana-ci-docker-rust-nightly).

## Moving to a newer nightly

We pin the version of nightly (see the `ARG nightly=xyz` line in `Dockerfile`)
to avoid the build breaking at unexpected times, as occasionally nightly will
introduce breaking changes.

To update the pinned version:
1. Locally update the `ARG nightly=xyz` line in `ci/docker-rust-nightly/Dockerfile`
1. Run `ci/docker-rust-nightly/build.sh` to rebuild the nightly image locally
1. Run `SOLANA_DOCKER_RUN_NOSETUID=1 ci/docker-run.sh --nopull solanalabs/rust-nightly ci/test-nightly.sh` to
   confirm the new nightly image builds.  Fix any issues as needed
1. Run `docker login` to enable pushing images to Docker Hub, if you're authorized.
1. Run `CI=true ci/docker-rust-nightly/build.sh` to push the new nightly image to dockerhub.com.
   **Note that all CI may be broken now until the next step is completed.**
1. Merge the update to `ARG nightly=xyz` and any codebase adjustments needed.


## Troubleshooting

### Resource is denied

When running `CI=true ci/docker-rust-nightly/build.sh`, you see:

```
denied: requested access to the resource is denied
```

Run `docker login` to enable pushing images to Docker Hub. Contact @mvines or @garious
to get write access.
