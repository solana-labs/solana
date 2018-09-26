// @flow

import {Account} from '../src/account';
import {BudgetProgram} from '../src/budget-program';

test('pay', () => {
  const from = new Account();
  const program = new Account();
  const to = new Account();
  let transaction;

  transaction = BudgetProgram.pay(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    123,
  );
  expect(transaction.keys).toHaveLength(2);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.pay(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
  );
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.pay(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
    BudgetProgram.timestampCondition(from.publicKey, new Date()),
  );
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.payOnBoth(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
    BudgetProgram.timestampCondition(from.publicKey, new Date()),
  );
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more
});

test('apply', () => {
  const from = new Account();
  const program = new Account();
  const to = new Account();
  let transaction;

  transaction = BudgetProgram.applyTimestamp(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    new Date(),
  );
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.applySignature(
    from.publicKey,
    program.publicKey,
    to.publicKey,
  );
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more
});

