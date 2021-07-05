/*
 Fetch the balance of an account
*/

//eslint-disable-next-line import/no-commonjs
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account = new solanaWeb3.Account();

let url;
url = 'http://devnet.safecoin.org';
//url = 'http://localhost:8328';
const connection = new solanaWeb3.Connection(url);

connection.getBalance(account.publicKey).then(balance => {
  console.log(`${account.publicKey} has a balance of ${balance}`);
});
