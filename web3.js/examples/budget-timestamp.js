/* eslint-disable import/no-commonjs */

/*
  Example of using the Budget program to perform a time-lock payment of 50
  lamports from account1 to account2.
*/

const common = require('./budget-common');
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account1 = new solanaWeb3.Account();
const account2 = new solanaWeb3.Account();
const contractState = new solanaWeb3.Account();

let url;
url = 'http://localhost:8899';
const connection = new solanaWeb3.Connection(url, 'recent');
const getTransactionFee = () => common.getTransactionFee(connection);
const showBalance = () =>
  common.showBalance(connection, account1, account2, contractState);
const confirmTransaction = signature =>
  common.confirmTransaction(connection, signature);
const airDrop = feeCalculator =>
  common.airDrop(connection, account1, feeCalculator);

getTransactionFee().then(feeCalculator => {
  airDrop(feeCalculator)
    .then(showBalance)
    .then(() => {
      console.log(`\n== Initializing contract`);
      const transaction = solanaWeb3.BudgetProgram.pay(
        account1.publicKey,
        contractState.publicKey,
        account2.publicKey,
        50,
        solanaWeb3.BudgetProgram.timestampCondition(
          account1.publicKey,
          new Date('2050'),
        ),
      );
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        account1,
        contractState,
      );
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
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        account1,
        contractState,
      );
    })
    .then(confirmTransaction)
    .then(showBalance)

    .then(() => {
      console.log('\nDone');
    })

    .catch(err => {
      console.log(err);
    });
});
