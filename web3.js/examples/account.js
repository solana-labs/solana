/*

 Creating a new account

 Usage:
 $ npm run dev
 $ node ./account.js

*/

//eslint-disable-next-line import/no-commonjs
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account = new solanaWeb3.Account();
console.log(account.publicKey);
