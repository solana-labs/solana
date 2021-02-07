/*
 Create a new account
*/

//eslint-disable-next-line import/no-commonjs
const safecoinWeb3 = require('..');
//const safecoinWeb3 = require('@safecoin/web3.js');

const account = new safecoinWeb3.Account();
console.log(account.publicKey.toString());
