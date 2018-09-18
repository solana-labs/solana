// @flow

import {Account} from '../src/account';
import {BudgetContract} from '../src/budget-contract';

test('pay', () => {
  const from = new Account();
  const contract = new Account();
  const to = new Account();
  let transaction;

  transaction = BudgetContract.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
  );
  console.log('Pay:', transaction);

  transaction = BudgetContract.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
    BudgetContract.signatureCondition(from.publicKey),
  );
  console.log('After:', transaction);

  transaction = BudgetContract.pay(
    from.publicKey,
    contract.publicKey,
    to.publicKey,
    123,
    BudgetContract.signatureCondition(from.publicKey),
    BudgetContract.timestampCondition(from.publicKey, new Date()),
  );
  console.log('Or:', transaction);
});

