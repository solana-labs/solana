// @flow

import {Account} from '../src/account';
import {BudgetProgram} from '../src/budget-program';

test('pay', () => {
  const from = new Account();
  const contract = new Account();
  const to = new Account();
  let transaction;

  transaction = BudgetProgram.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
  );
  console.log('Pay:', transaction);

  transaction = BudgetProgram.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
  );
  console.log('After:', transaction);

  transaction = BudgetProgram.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
    BudgetProgram.signatureCondition(from.publicKey),
    BudgetProgram.timestampCondition(from.publicKey, new Date()),
  );
  console.log('Or:', transaction);
});

