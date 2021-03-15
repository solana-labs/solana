[![Build status][travis-image]][travis-url]
[![codecov][codecov-image]][codecov-url]
<br>
[![npm][npm-image]][npm-url]
[![npm-downloads][npm-downloads-image]][npm-url]
<br>
[![semantic-release][semantic-release-image]][semantic-release-url]
[![code-style-prettier][code-style-prettier-image]][code-style-prettier-url]

[travis-image]: https://api.travis-ci.org/solana-labs/solana-web3.js.svg?branch=master
[travis-url]: https://travis-ci.org/solana-labs/solana-web3.js
[codecov-image]: https://codecov.io/gh/solana-labs/solana-web3.js/branch/master/graph/badge.svg
[codecov-url]: https://codecov.io/gh/solana-labs/solana-web3.js
[npm-image]: https://img.shields.io/npm/v/@solana/web3.js.svg?style=flat
[npm-downloads-image]: https://img.shields.io/npm/dm/@solana/web3.js.svg?style=flat
[npm-url]: https://www.npmjs.com/package/@solana/web3.js
[semantic-release-image]: https://img.shields.io/badge/%20%20%F0%9F%93%A6%F0%9F%9A%80-semantic--release-e10079.svg
[semantic-release-url]: https://github.com/semantic-release/semantic-release
[code-style-prettier-image]: https://img.shields.io/badge/code_style-prettier-ff69b4.svg?style=flat-square
[code-style-prettier-url]: https://github.com/prettier/prettier

# Solana JavaScript API

This is the Solana Javascript API built on the Solana [JSON RPC API](https://docs.solana.com/apps/jsonrpc-api)

[Latest API Documentation](https://solana-labs.github.io/solana-web3.js/)


## Installation

### Yarn
```
$ yarn add @solana/web3.js
```

### npm
```
$ npm install --save @solana/web3.js
```

### Browser bundle
```html
<!-- Development (un-minified) -->
<script src="https://unpkg.com/@solana/web3.js@0.92.0/lib/index.iife.js"></script>

<!-- Production (un-minified) -->
<script src="https://unpkg.com/@solana/web3.js@0.92.0/lib/index.iife.min.js"></script>
```


### Development Environment Setup
To build and run tests:
1. Install Rust from https://rustup.rs/
2. Install the latest Solana release from https://docs.solana.com/cli/install-solana-cli-tools

## Usage

### Javascript
```js
const solanaWeb3 = require('@solana/web3.js');
console.log(solanaWeb3);
```

### ES6
```js
import solanaWeb3 from '@solana/web3.js';
console.log(solanaWeb3);
```

### Browser bundle
```js
// `solanaWeb3` is provided in the global namespace by the `solanaWeb3.min.js` script bundle.
console.log(solanaWeb3);
```

## Flow

A [Flow library definition](https://flow.org/en/docs/libdefs/) is provided at
[module.flow.js](https://github.com/solana-labs/solana-web3.js/tree/master/module.flow.js).
Add the following line under the [libs] section of your project's .flowconfig to
activate it:
```ini
[libs]
node_modules/@solana/web3.js/module.flow.js
```

## Examples
See the [examples/](https://github.com/solana-labs/solana-web3.js/tree/master/examples) directory for small snippets.

Standalone examples:
* Message feed (BPF Rust and C): https://github.com/solana-labs/example-messagefeed
* Tic-tac-toe (BPF C): https://github.com/solana-labs/example-tictactoe
* Web wallet: https://github.com/solana-labs/example-webwallet

## Releases
Releases are available on [Github](https://github.com/solana-labs/solana-web3.js/releases)
and [npmjs.com](https://www.npmjs.com/package/@solana/web3.js)

Each Github release features a tarball containing API documentation and a
minified version of the module suitable for direct use in a browser environment
(&lt;script&gt; tag)

## Deprecated

### Local Network

**Please use `solana-test-validator` from the latest Solana release instead of the information in this section**

The `solana-localnet` program is provided to easily start a test Solana cluster
locally on your machine.  Docker must be installed.  The JSON RPC endpoint of
the local cluster is `http://localhost:8899`.

To start, first fetch the latest Docker image by running:
```bash
$ npx solana-localnet update
```

Then run the following command to start the cluster
```bash
$ npx solana-localnet up
```

While the cluster is running logs are available with:
```bash
$ npx solana-localnet logs -f
```

Stop the cluster with:
```bash
$ npx solana-localnet down
```

### BPF program development

**Please use `cargo build-bpf` from the latest Solana release instead of the information in this section**

The Solana BPF SDK is located in the `bpf-sdk/` subdirectory if you installed
solana-web3.js from npmjs.com.

From a git clone, run `npm run bpf-sdk:install` to fetch the latest BPF SDK.

Additionally Rust must be installed to build Rust BPF programs such as
`examples/bpf-rust-noop/`.  See https://www.rust-lang.org/install.html for
installation details.
