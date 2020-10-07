
# Solana WebAssembly API

@solana/wasm exposes Ed25519 and key generation functions to javascript clients. 

## Installation

### Yarn
```
$ yarn add @solana/wasm
```

### npm
```
$ npm install --save @solana/wasm
```

## Usage

### Node.JS

```
const sdk = require('@solana/wasm');
const TextEncoder = require('util').TextEncoder;

(async () => {
    await sdk.waitReady();
    const keyPair = sdk.ed25519.keypair.generate();
    const signature = sdk.ed25519.sign(keyPair.publicKey, keyPair.secretKey, new TextEncoder().encode('message to encode'));
    console.log('KeyPair and Signature', {
        keyPair, 
        signature
    })

    console.log('KeyPair from secret key', sdk.ed25519.keypair.fromSecretKey(keyPair.secretKey));

    console.log('Sha256', sdk.hasher.sha256(signature));
})();

```

### Webpack/CRA

Use customize-cra to customize default config. Add code below to config-overrides.js

```
const path = require("path");
const { override, addExternalBabelPlugins } = require("customize-cra");
const WASM = require('@solana/wasm/webpack');

module.exports = override(
  ...addExternalBabelPlugins(WASM.BabelPlugin),
  (config) =>{
    config.module.rules.forEach((rule) => {
      (rule.oneOf || []).forEach((oneOf) => {
        if (oneOf.loader && oneOf.loader.indexOf("file-loader") >= 0) {
          // Make file-loader ignore WASM files
          oneOf.exclude.push(/\.wasm$/);
        }
      });
    });

    config.plugins.push(new WASM.WebpackPlugin());

    return config;
  },
);
```

Yu can then start using @solana/wasm inside JavaScript code
```
import { waitReady, ed25519 } from '@solana/wasm';

// before any calls to api are made you need to await 'waitReady' function
waitReady().then(() => {
    const keyPair = ed25519.keypair.generate();
    const signature = ed25519.sign(keyPair.public, keyPair.secret, new TextEncoder().encode('message to encode'));
    console.log(signature);
});
```

## Building

Requires:
* rustup
* wasm-pack
* Node.js >= 12

Run `npm run build`