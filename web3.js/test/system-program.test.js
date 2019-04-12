// @flow

import {Account, BudgetProgram, SystemProgram} from '../src';

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

test('transfer', () => {
  const from = new Account();
  const to = new Account();
  let transaction;

  transaction = SystemProgram.transfer(from.publicKey, to.publicKey, 123);

  expect(transaction.keys).toHaveLength(2);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});

test('assign', () => {
  const from = new Account();
  const to = new Account();
  let transaction;

  transaction = SystemProgram.assign(from.publicKey, to.publicKey);

  expect(transaction.keys).toHaveLength(1);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});
