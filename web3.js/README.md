[![Build status][travis-image]][travis-url]
[![codecov][codecov-image]][codecov-url]
[![esdoc][esdoc-image]][esdoc-url]
<br>
[![npm][npm-image]][npm-url]
[![npm-downloads][npm-downloads-image]][npm-url]
[![david][david-deps-image]][david-deps-url]
[![david][david-dev-deps-image]][david-dev-deps-url]
[![semantic-release][semantic-release-image]][semantic-release-url]

[travis-image]: https://api.travis-ci.org/solana-labs/solana-web3.js.svg?branch=master
[travis-url]: https://travis-ci.org/solana-labs/solana-web3.js
[codecov-image]: https://codecov.io/gh/solana-labs/solana-web3.js/branch/master/graph/badge.svg
[codecov-url]: https://codecov.io/gh/solana-labs/solana-web3.js
[esdoc-image]: https://solana-labs.github.io/solana-web3.js/badge.svg
[npm-image]: https://img.shields.io/npm/v/@solana/web3.js.svg?style=flat
[npm-downloads-image]: https://img.shields.io/npm/dm/@solana/web3.js.svg?style=flat
[esdoc-url]: https://solana-labs.github.io/solana-web3.js/
[npm-url]: https://www.npmjs.com/package/@solana/web3.js
[david-deps-image]: https://david-dm.org/solana-labs/solana-web3.js.svg
[david-deps-url]: https://david-dm.org/solana-labs/solana-web3.js
[david-dev-deps-image]: https://david-dm.org/solana-labs/solana-web3.js/dev-status.svg
[david-dev-deps-url]: https://david-dm.org/solana-labs/solana-web3.js?type=dev
[semantic-release-image]: https://img.shields.io/badge/%20%20%F0%9F%93%A6%F0%9F%9A%80-semantic--release-e10079.svg
[semantic-release-url]: https://github.com/semantic-release/semantic-release

# Solana JavaScript API

This is the Solana Javascript API built on the Solana [JSON RPC API](https://github.com/solana-labs/solana/blob/master/doc/json-rpc.md)

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
<script src="https://github.com/solana-labs/solana-web3.js/releases/download/v0.0.6/solanaWeb3.min.js"></script>
```

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
* Web wallet: https://github.com/solana-labs/example-webwallet
* Tic-tac-toe: https://github.com/solana-labs/example-tictactoe

## Releases
Releases are available on [Github](https://github.com/solana-labs/solana-web3.js/releases)
and [npmjs.com](https://www.npmjs.com/package/@solana/web3.js)

Each Github release features a tarball containing API documentation and a
minified version of the module suitable for direct use in a browser environment
(&lt;script&gt; tag)
