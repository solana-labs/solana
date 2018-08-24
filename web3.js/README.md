[![Build status](https://api.travis-ci.org/solana-labs/solana-web3.js.svg?branch=master)](https://travis-ci.org/solana-labs/solana-web3.js)
[![codecov](https://codecov.io/gh/solana-labs/solana-web3.js/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana-web3.js)
[![esdoc](https://solana-labs.github.io/solana-web3.js/badge.svg)](https://solana-labs.github.io/solana-web3.js/)
[![npm](https://img.shields.io/npm/v/@solana/web3.js.svg?style=flat)](https://www.npmjs.com/package/@solana/web3.js)

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
<script src="https://github.com/solana-labs/solana-web3.js/releases/download/v0.0.3/solanaWeb3.min.js"></script>
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

## Examples
See the [examples/](https://github.com/solana-labs/solana-web3.js/tree/master/examples) directory for small snippets

A more complete web-based wallet example can be found at https://github.com/solana-labs/example-webwallet

## Releases
Releases are available on [Github](https://github.com/solana-labs/solana-web3.js/releases)
and [npmjs.com](https://www.npmjs.com/package/@solana/web3.js)

Each Github release features a tarball containing API documentation and a
minified version of the module suitable for direct use in a browser environment
(&lt;script&gt; tag)
