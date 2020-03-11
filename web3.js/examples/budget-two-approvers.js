/* eslint-disable import/no-commonjs */

/*
  Example of using the Budget program to perform a payment authorized by two parties
*/

const common = require('./budget-common');

const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account1 = new solanaWeb3.Account();
const account2 = new solanaWeb3.Account();
const contractState = new solanaWeb3.Account();

const approver1 = new solanaWeb3.Account();
const approver2 = new solanaWeb3.Account();

let url;
url = 'http://localhost:8899';
//url = 'http://devnet.solana.com';
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
    .then(() => {
      console.log(`\n== Move 1 lamport to approver1`);
      const transaction = solanaWeb3.SystemProgram.transfer(
        account1.publicKey,
        approver1.publicKey,
        1 + feeCalculator.lamportsPerSignature,
      );
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        account1,
      );
    })
    .then(confirmTransaction)
    .then(getTransactionFee)
    .then(() => {
      console.log(`\n== Move 1 lamport to approver2`);
      const transaction = solanaWeb3.SystemProgram.transfer(
        account1.publicKey,
        approver2.publicKey,
        1 + feeCalculator.lamportsPerSignature,
      );
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        account1,
      );
    })
    .then(confirmTransaction)
    .then(showBalance)
    .then(() => {
      console.log(`\n== Initializing contract`);
      const transaction = solanaWeb3.BudgetProgram.payOnBoth(
        account1.publicKey,
        contractState.publicKey,
        account2.publicKey,
        50,
        solanaWeb3.BudgetProgram.signatureCondition(approver1.publicKey),
        solanaWeb3.BudgetProgram.signatureCondition(approver2.publicKey),
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
      console.log(`\n== Apply approver 1`);
      const transaction = solanaWeb3.BudgetProgram.applySignature(
        approver1.publicKey,
        contractState.publicKey,
        account2.publicKey,
      );
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        approver1,
      );
    })
    .then(confirmTransaction)
    .then(showBalance)
    .then(() => {
      console.log(`\n== Apply approver 2`);
      const transaction = solanaWeb3.BudgetProgram.applySignature(
        approver2.publicKey,
        contractState.publicKey,
        account2.publicKey,
      );
      return solanaWeb3.sendAndConfirmTransaction(
        connection,
        transaction,
        approver2,
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
