/*
 Fetch the balance of an account
*/

//eslint-disable-next-line import/no-commonjs
const safecoinWeb3 = require('..');
//const safecoinWeb3 = require('@safecoin/web3.js');

const account = new safecoinWeb3.Account();

let url;
url = 'http://devnet.safecoin.org';
//url = 'http://localhost:8899';
const connection = new safecoinWeb3.Connection(url);

connection.getBalance(account.publicKey).then(balance => {
  console.log(`${account.publicKey} has a balance of ${balance}`);
});
