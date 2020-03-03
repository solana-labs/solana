// @flow
import {Account} from '../src/account';
import {SystemProgram} from '../src/system-program';
import {Transaction} from '../src/transaction';

test('verify getConfirmedBlock', () => {
  const account0 = new Account();
  const account1 = new Account();
  const account2 = new Account();
  const account3 = new Account();
  const recentBlockhash = account1.publicKey.toBase58(); // Fake recentBlockhash

  // Create a couple signed transactions
  const transfer0 = SystemProgram.transfer({
    fromPubkey: account0.publicKey,
    toPubkey: account1.publicKey,
    lamports: 123,
  });

  const transaction0 = new Transaction({recentBlockhash}).add(transfer0);
  transaction0.sign(account0);
  const transfer1 = SystemProgram.transfer({
    fromPubkey: account2.publicKey,
    toPubkey: account3.publicKey,
    lamports: 456,
  });

  let transaction1 = new Transaction({recentBlockhash}).add(transfer1);
  transaction1.sign(account2);

  // Build ConfirmedBlock, with dummy data for blockhashes, balances
  const confirmedBlock = {
    blockhash: recentBlockhash,
    previousBlockhash: recentBlockhash,
    transactions: [
      {
        transaction: transaction0,
        meta: {
          fee: 0,
          preBalances: [100000, 100000, 1, 1, 1],
          postBalances: [99877, 100123, 1, 1, 1],
          status: {Ok: 'null'},
        },
      },
      {
        transaction: transaction1,
        meta: {
          fee: 0,
          preBalances: [100000, 100000, 1, 1, 1],
          postBalances: [99544, 100456, 1, 1, 1],
          status: {Ok: 'null'},
        },
      },
    ],
    rewards: [],
  };

  // Verify signatures in ConfirmedBlock
  for (const transactionWithMeta of confirmedBlock.transactions) {
    expect(transactionWithMeta.transaction.verifySignatures()).toBe(true);
  }

  const bogusSignature = {
    signature: Buffer.alloc(64, 9),
    publicKey: account2.publicKey,
  };
  transaction1.signatures[0] = bogusSignature;

  let badConfirmedBlock = confirmedBlock;
  badConfirmedBlock.transactions[1].transaction = transaction1;

  // Verify signatures in ConfirmedBlock
  const verifications = badConfirmedBlock.transactions.map(
    transactionWithMeta => transactionWithMeta.transaction.verifySignatures(),
  );
  expect(
    verifications.reduce(
      (accumulator, currentValue) => accumulator && currentValue,
    ),
  ).toBe(false);
});
