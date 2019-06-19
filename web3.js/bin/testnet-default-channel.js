#!/usr/bin/env node

let p = [
  __dirname + '/../lib/node_modules/@solana/web3.js/package.json',
  __dirname + '/../@solana/web3.js/package.json',
  __dirname + '/../package.json'
].find(require('fs').existsSync);
if (!p) throw new Error('Unable to locate solana-web3.js directory');

console.log(require(p)['testnetDefaultChannel']);
