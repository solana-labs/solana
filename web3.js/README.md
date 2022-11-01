[![codecov][codecov-image]][codecov-url]
<br>
[![npm][npm-image]][npm-url]
[![npm-downloads][npm-downloads-image]][npm-url]
<br>
[![semantic-release][semantic-release-image]][semantic-release-url]
[![code-style-prettier][code-style-prettier-image]][code-style-prettier-url]

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

## Documentation and examples

 - [The Solana Cookbook](https://solanacookbook.com/) has extensive task-based documentation using this library.
 - For more detail on individual functions, see the [latest API Documentation](https://solana-labs.github.io/solana-web3.js/)

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

<!-- Production (minified) -->
<script src="https://unpkg.com/@solana/web3.js@latest/lib/index.iife.min.js"></script>
```

## Development Environment Setup

Install the latest Solana release from https://docs.solana.com/cli/install-solana-cli-tools

### Run test validator

**Use `solana-test-validator` from the latest Solana release**

### SBF program development

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

## Compatibility

This library requires a JavaScript runtime that supports [`BigInt`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/BigInt) and the [exponentiation operator](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/Exponentiation). Both are supported in the following runtimes:

- Browsers, by [release date](https://caniuse.com/bigint):
  - Chrome: May 2018
  - Firefox: July 2019
  - Safari: September 2020
  - Mobile Safari: September 2020
  - Edge: January 2020
  - Opera: June 2018
  - Samsung Internet: April 2019
- Runtimes, [by version](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/BigInt):
  - Deno: >=1.0
  - Node: >=10.4.0
- React Native:
  - \>=0.7.0 using the [Hermes](https://reactnative.dev/blog/2022/07/08/hermes-as-the-default) engine ([integration guide](https://solanacookbook.com/integrations/react-native.html#how-to-use-solana-web3-js-in-a-react-native-app)):

## Flow Support (Discontinued)

Flow types are no longer supported in new releases. The last release with Flow support is v1.37.2 and its
[Flow library definition](https://flow.org/en/docs/libdefs/) is provided at
https://unpkg.com/@solana/web3.js@v1.37.2/module.flow.js.
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

## Contributing

If you have an issue to report or would like to contribute a pull request, please do so against the monorepo at https://github.com/solana-labs/solana. We are not able to merge pull requests into the mirror repo https://github.com/solana-labs/solana-web3.js and issues filed there may go unnoticed.

## Disclaimer

All claims, content, designs, algorithms, estimates, roadmaps,
specifications, and performance measurements described in this project
are done with the Solana Foundation's ("SF") best efforts. It is up to
the reader to check and validate their accuracy and truthfulness.
Furthermore nothing in this project constitutes a solicitation for
investment.

Any content produced by SF or developer resources that SF provides, are
for educational and inspiration purposes only. SF does not encourage,
induce or sanction the deployment, integration or use of any such
applications (including the code comprising the Solana blockchain
protocol) in violation of applicable laws or regulations and hereby
prohibits any such deployment, integration or use. This includes use of
any such applications by the reader (a) in violation of export control
or sanctions laws of the United States or any other applicable
jurisdiction, (b) if the reader is located in or ordinarily resident in
a country or territory subject to comprehensive sanctions administered
by the U.S. Office of Foreign Assets Control (OFAC), or (c) if the
reader is or is working on behalf of a Specially Designated National
(SDN) or a person subject to similar blocking or denied party
prohibitions.

The reader should be aware that U.S. export control and sanctions laws
prohibit U.S. persons (and other persons that are subject to such laws)
from transacting with persons in certain countries and territories or
that are on the SDN list. As a project based primarily on open-source
software, it is possible that such sanctioned persons may nevertheless
bypass prohibitions, obtain the code comprising the Solana blockchain
protocol (or other project code or applications) and deploy, integrate,
or otherwise use it. Accordingly, there is a risk to individuals that
other persons using the Solana blockchain protocol may be sanctioned
persons and that transactions with such persons would be a violation of
U.S. export controls and sanctions law. This risk applies to
individuals, organizations, and other ecosystem participants that
deploy, integrate, or use the Solana blockchain protocol code directly
(e.g., as a node operator), and individuals that transact on the Solana
blockchain through light clients, third party interfaces, and/or wallet
software.
