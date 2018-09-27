/*
  Example of using the Budget program to perform a time-lock payment of 50
  tokens from account1 to account2.
*/

//eslint-disable-next-line import/no-commonjs
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account1 = new solanaWeb3.Account();
const account2 = new solanaWeb3.Account();
const contractFunds = new solanaWeb3.Account();
const contractState = new solanaWeb3.Account();

let url;
url = 'http://localhost:8899';
//url = 'https://api.testnet.solana.com/master';
//url = 'https://api.testnet.solana.com';
const connection = new solanaWeb3.Connection(url);

function showBalance() {
  console.log(`\n== Account State`);
  return Promise.all([
    connection.getBalance(account1.publicKey),
    connection.getBalance(account2.publicKey),
    connection.getBalance(contractFunds.publicKey),
    connection.getBalance(contractState.publicKey),
  ]).then(([fromBalance, toBalance, contractFundsBalance, contractStateBalance]) => {
    console.log(`Account1:       ${account1.publicKey} has a balance of ${fromBalance}`);
    console.log(`Account2:       ${account2.publicKey} has a balance of ${toBalance}`);
    console.log(`Contract Funds: ${contractFunds.publicKey} has a balance of ${contractFundsBalance}`);
    console.log(`Contract State: ${contractState.publicKey} has a balance of ${contractStateBalance}`);
  });
}

function confirmTransaction(signature) {
  console.log('Confirming transaction:', signature);
  return connection.getSignatureStatus(signature)
  .then((confirmation) => {
    if (confirmation !== 'Confirmed') {
      throw new Error(`Transaction was not confirmed (${confirmation})`);
    }
    console.log('Transaction confirmed');
  });
}

function airDrop() {
  console.log(`\n== Requesting airdrop of 100 to ${account1.publicKey}`);
  return connection.requestAirdrop(account1.publicKey, 100)
  .then(confirmTransaction);
}

showBalance()
.then(airDrop)
.then(showBalance)
.then(() => {
  console.log(`\n== Creating account for the contract funds`);
  const transaction = solanaWeb3.SystemProgram.createAccount(
    account1.publicKey,
    contractFunds.publicKey,
    50, // number of tokens to transfer
    0,
    solanaWeb3.BudgetProgram.programId,
  );
  return connection.sendTransaction(account1, transaction);
})
.then(confirmTransaction)
.then(showBalance)
.then(() => {
  console.log(`\n== Creating account for the contract state`);
  const transaction = solanaWeb3.SystemProgram.createAccount(
    account1.publicKey,
    contractState.publicKey,
    1, // account1 pays 1 token to hold the contract state
    solanaWeb3.BudgetProgram.space,
    solanaWeb3.BudgetProgram.programId,
  );
  return connection.sendTransaction(account1, transaction);
})
.then(confirmTransaction)
.then(showBalance)
.then(() => {
  console.log(`\n== Initializing contract`);
  const transaction = solanaWeb3.BudgetProgram.pay(
    contractFunds.publicKey,
    contractState.publicKey,
    account2.publicKey,
    50,
    solanaWeb3.BudgetProgram.timestampCondition(account1.publicKey, new Date('2050')),
  );
  return connection.sendTransaction(contractFunds, transaction);
})
.then(confirmTransaction)
.then(showBalance)
.then(() => {
  console.log(`\n== Witness contract`);
  const transaction = solanaWeb3.BudgetProgram.applyTimestamp(
    account1.publicKey,
    contractState.publicKey,
    account2.publicKey,
    new Date('2050'),
  );
  return connection.sendTransaction(account1, transaction);
})
.then(confirmTransaction)
.then(showBalance)

.then(() => {
  console.log('\nDone');
})

.catch((err) => {
  console.log(err);
});
