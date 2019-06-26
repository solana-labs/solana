/*
  Example of using the Budget program to perform a payment authorized by two parties
*/

//eslint-disable-next-line import/no-commonjs
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account1 = new solanaWeb3.Account();
const account2 = new solanaWeb3.Account();
const contractState = new solanaWeb3.Account();

const approver1 = new solanaWeb3.Account();
const approver2 = new solanaWeb3.Account();

let url;
url = 'http://localhost:8899';
//url = 'http://testnet.solana.com:8899';
const connection = new solanaWeb3.Connection(url);

function showBalance() {
  console.log(`\n== Account State`);
  return Promise.all([
    connection.getBalance(account1.publicKey),
    connection.getBalance(account2.publicKey),
    connection.getBalance(contractState.publicKey),
  ]).then(([fromBalance, toBalance, contractStateBalance]) => {
    console.log(
      `Account1:       ${account1.publicKey} has a balance of ${fromBalance}`,
    );
    console.log(
      `Account2:       ${account2.publicKey} has a balance of ${toBalance}`,
    );
    console.log(
      `Contract State: ${contractState.publicKey} has a balance of ${contractStateBalance}`,
    );
  });
}

function confirmTransaction(signature) {
  console.log('Confirming transaction:', signature);
  return connection.getSignatureStatus(signature).then(confirmation => {
    if (confirmation && 'Ok' in confirmation) {
      console.log('Transaction confirmed');
    } else if (confirmation) {
      throw new Error(
        `Transaction was not confirmed (${JSON.stringify(confirmation.Err)})`,
      );
    } else {
      throw new Error(`Transaction was not confirmed (${confirmation})`);
    }
  });
}

function airDrop() {
  console.log(`\n== Requesting airdrop of 100 to ${account1.publicKey}`);
  return connection
    .requestAirdrop(account1.publicKey, 100)
    .then(confirmTransaction);
}

showBalance()
  .then(airDrop)
  .then(() => {
    console.log(`\n== Move 1 lamport to approver1`);
    const transaction = solanaWeb3.SystemProgram.transfer(
      account1.publicKey,
      approver1.publicKey,
      1,
    );
    return connection.sendTransaction(transaction, account1);
  })
  .then(confirmTransaction)
  .then(() => {
    console.log(`\n== Move 1 lamport to approver2`);
    const transaction = solanaWeb3.SystemProgram.transfer(
      account1.publicKey,
      approver2.publicKey,
      1,
    );
    return connection.sendTransaction(transaction, account1);
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
