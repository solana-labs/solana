// @flow
import {Account} from '../src/account';
import {Transaction} from '../src/transaction';
import {SystemProgram} from '../src/system-program';

test('signPartial', () => {
  const account1 = new Account();
  const account2 = new Account();
  const lastId = account1.publicKey.toBase58(); // Fake lastId
  const move = SystemProgram.move(account1.publicKey, account2.publicKey, 123);

  const transaction = new Transaction({lastId}).add(move);
  transaction.sign(account1, account2);

  const partialTransaction = new Transaction({lastId}).add(move);
  partialTransaction.signPartial(account1, account2.publicKey);
  expect(partialTransaction.signatures[1].signature).toBeNull();
  partialTransaction.addSigner(account2);

  expect(partialTransaction).toEqual(transaction);
});

test('transfer signatures', () => {
  const account1 = new Account();
  const account2 = new Account();
  const lastId = account1.publicKey.toBase58(); // Fake lastId
  const move1 = SystemProgram.move(account1.publicKey, account2.publicKey, 123);
  const move2 = SystemProgram.move(account2.publicKey, account1.publicKey, 123);

  const orgTransaction = new Transaction({lastId}).add(move1, move2);
  orgTransaction.sign(account1, account2);

  const newTransaction = new Transaction({
    lastId: orgTransaction.lastId,
    fee: orgTransaction.fee,
    signatures: orgTransaction.signatures,
  }).add(move1, move2);

  expect(newTransaction).toEqual(orgTransaction);
});
