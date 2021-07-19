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
<script src="https://unpkg.com/@solana/web3.js@latest/lib/index.iife.js"></script>

<!-- Production (un-minified) -->
<script src="https://unpkg.com/@solana/web3.js@latest/lib/index.iife.min.js"></script>
```

## Development Environment Setup

Install the latest Solana release from https://docs.solana.com/cli/install-solana-cli-tools

### Run test validator

**Use `solana-test-validator` from the latest Solana release**

### BPF program development

**Use `cargo build-bpf` from the latest Solana release**

## Usage

### Javascript

```js
const solanaWeb3 = require('@solana/web3.js');
console.log(solanaWeb3);
```

### ES6

```js
import * as solanaWeb3 from '@solana/web3.js';
console.log(solanaWeb3);
```

### Browser bundle

```js
// `solanaWeb3` is provided in the global namespace by the `solanaWeb3.min.js` script bundle.
console.log(solanaWeb3);
```

## Examples

Example scripts for the web3.js repo and native programs:

- [Web3 Examples](./examples)

Example scripts for the Solana Program Library:

- [Token Program Examples](https://github.com/solana-labs/solana-program-library/tree/master/token/js/examples)

## Flow

A [Flow library definition](https://flow.org/en/docs/libdefs/) is provided at
https://unpkg.com/@solana/web3.js@latest/module.flow.js.
Download the file and add the following line under the [libs] section of your project's `.flowconfig` to
activate it:

```ini
[libs]
node_modules/@solana/web3.js/module.flow.js
```

## Releases

Releases are available on [Github](https://github.com/solana-labs/solana-web3.js/releases)
and [npmjs.com](https://www.npmjs.com/package/@solana/web3.js)

Each Github release features a tarball containing API documentation and a
minified version of the module suitable for direct use in a browser environment
(`<script>` tag)
