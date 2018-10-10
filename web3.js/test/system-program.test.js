// @flow

import {
  Account,
  BudgetProgram,
  Connection,
  SystemProgram,
  Transaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithTokens} from './new-account-with-tokens';

test('createAccount', () => {
  const from = new Account();
  const newAccount = new Account();
  let transaction;

  transaction = SystemProgram.createAccount(
    from.publicKey,
    newAccount.publicKey,
    123,
    BudgetProgram.space,
    BudgetProgram.programId,
  );

  expect(transaction.keys).toHaveLength(2);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});

test('move', () => {
  const from = new Account();
  const to = new Account();
  let transaction;

  transaction = SystemProgram.move(
    from.publicKey,
    to.publicKey,
    123,
  );

  expect(transaction.keys).toHaveLength(2);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});


test('assign', () => {
  const from = new Account();
  const to = new Account();
  let transaction;

  transaction = SystemProgram.assign(
    from.publicKey,
    to.publicKey,
  );

  expect(transaction.keys).toHaveLength(1);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});

test('unstable - load', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const from = await newAccountWithTokens(connection);
  const noopProgramId = (new Account()).publicKey;

  const loadTransaction = SystemProgram.load(
    from.publicKey,
    noopProgramId,
    'noop',
  );

  let signature = await connection.sendTransaction(from, loadTransaction);
  expect(connection.confirmTransaction(signature)).resolves.toBe(true);

  const noopTransaction = new Transaction({
    fee: 0,
    keys: [from.publicKey],
    programId: noopProgramId,
  });
  signature = await connection.sendTransaction(from, noopTransaction);
  expect(connection.confirmTransaction(signature)).resolves.toBe(true);


});

