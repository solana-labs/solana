/* eslint-disable import/no-commonjs */

/*
  Common code for the budget program examples
*/

function getTransactionFee(connection) {
  return connection.getRecentBlockhash().then(response => {
    return response[1];
  });
}

function showBalance(connection, account1, account2, contractState) {
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

function confirmTransaction(connection, signature) {
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

function airDrop(connection, account, feeCalculator) {
  const airdrop = 100 + 5 * feeCalculator.targetLamportsPerSignature;
  console.log(`\n== Requesting airdrop of ${airdrop} to ${account.publicKey}`);
  return connection
    .requestAirdrop(account.publicKey, airdrop)
    .then(signature => confirmTransaction(connection, signature));
}

function sleep(millis) {
  return new Promise(resolve => {
    setTimeout(resolve, millis);
  });
}

module.exports = {
  airDrop,
  confirmTransaction,
  getTransactionFee,
  showBalance,
  sleep,
};
