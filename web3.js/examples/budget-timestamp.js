/*
  Example of using the Budget program to perform a time-lock payment of 50
  lamports from account1 to account2.
*/

//eslint-disable-next-line import/no-commonjs
const solanaWeb3 = require('..');
//const solanaWeb3 = require('@solana/web3.js');

const account1 = new solanaWeb3.Account();
const account2 = new solanaWeb3.Account();
const contractState = new solanaWeb3.Account();

let url;
url = 'http://localhost:8899';
const connection = new solanaWeb3.Connection(url, 'recent');

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
  console.log(`\n== Requesting airdrop of 100000 to ${account1.publicKey}`);
  return connection
    .requestAirdrop(account1.publicKey, 100000)
    .then(confirmTransaction);
}

showBalance()
  .then(airDrop)
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
