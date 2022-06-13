
## BigTable Setup

### Development Environment
The Cloud BigTable emulator can be used during development/test.  See
https://cloud.google.com/bigtable/docs/emulator for general setup information.

Process:
1. Run `gcloud beta emulators bigtable start` in the background
2. Run `$(gcloud beta emulators bigtable env-init)` to establish the `BIGTABLE_EMULATOR_HOST` environment variable
3. Run `./init-bigtable.sh` to configure the emulator
4. Develop/test

### Production Environment
Export a standard `GOOGLE_APPLICATION_CREDENTIALS` environment variable to your
service account credentials.  The project should contain a BigTable instance
called `solana-ledger` that has been initialized by running the `./init-bigtable.sh` script.

Depending on what operation mode is required, either the
`https://www.googleapis.com/auth/bigtable.data` or
`https://www.googleapis.com/auth/bigtable.data.readonly` OAuth scope will be
requested using the provided credentials.

#### Forward proxy
Export `BIGTABLE_PROXY` environment variable for the forward proxy as you would
for `HTTP_PROXY`. This will establish a tunnel through the forward proxy for
gRPC traffic (the tunneled traffic will still use TLS as normal).
