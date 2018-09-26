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
  console.log('Pay:', transaction);
  expect(transaction.keys).toHaveLength(2);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.pay(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
  );
  console.log('After:', transaction);
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
  console.log('Or:', transaction);
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
  console.log('applyTimestamp:', transaction);
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more

  transaction = BudgetProgram.applySignature(
    from.publicKey,
    program.publicKey,
    to.publicKey,
  );
  console.log('applySignature:', transaction);
  expect(transaction.keys).toHaveLength(3);
  // TODO: Validate transaction contents more
});

