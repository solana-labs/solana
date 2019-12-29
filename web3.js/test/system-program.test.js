// @flow

import {
  Account,
  BudgetProgram,
  SystemInstruction,
  SystemProgram,
  Transaction,
} from '../src';

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

test('createAccountWithSeed', () => {
  const from = new Account();
  const newAccount = new Account();
  let transaction;

  transaction = SystemProgram.createAccountWithSeed(
    from.publicKey,
    newAccount.publicKey,
    from.publicKey,
    'hi there',
    123,
    BudgetProgram.space,
    BudgetProgram.programId,
  );

  expect(transaction.keys).toHaveLength(2);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});

test('SystemInstruction create', () => {
  const from = new Account();
  const to = new Account();
  const program = new Account();
  const amount = 42;
  const space = 100;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const create = SystemProgram.createAccount(
    from.publicKey,
    to.publicKey,
    amount,
    space,
    program.publicKey,
  );
  const transaction = new Transaction({recentBlockhash}).add(create);

  const systemInstruction = SystemInstruction.from(transaction.instructions[0]);
  expect(systemInstruction.fromPublicKey).toEqual(from.publicKey);
  expect(systemInstruction.toPublicKey).toEqual(to.publicKey);
  expect(systemInstruction.amount).toEqual(amount);
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);
});

test('SystemInstruction transfer', () => {
  const from = new Account();
  const to = new Account();
  const amount = 42;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const transfer = SystemProgram.transfer(from.publicKey, to.publicKey, amount);
  const transaction = new Transaction({recentBlockhash}).add(transfer);
  transaction.sign(from);

  const systemInstruction = SystemInstruction.from(transaction.instructions[0]);
  expect(systemInstruction.fromPublicKey).toEqual(from.publicKey);
  expect(systemInstruction.toPublicKey).toEqual(to.publicKey);
  expect(systemInstruction.amount).toEqual(amount);
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);
});

test('SystemInstruction assign', () => {
  const from = new Account();
  const program = new Account();
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const assign = SystemProgram.assign(from.publicKey, program.publicKey);
  const transaction = new Transaction({recentBlockhash}).add(assign);
  transaction.sign(from);

  const systemInstruction = SystemInstruction.from(transaction.instructions[0]);
  expect(systemInstruction.fromPublicKey).toBeNull();
  expect(systemInstruction.toPublicKey).toBeNull();
  expect(systemInstruction.amount).toBeNull();
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);
});

test('SystemInstruction createWithSeed', () => {
  const from = new Account();
  const to = new Account();
  const program = new Account();
  const amount = 42;
  const space = 100;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const create = SystemProgram.createAccountWithSeed(
    from.publicKey,
    to.publicKey,
    from.publicKey,
    'hi there',
    amount,
    space,
    program.publicKey,
  );
  const transaction = new Transaction({recentBlockhash}).add(create);

  const systemInstruction = SystemInstruction.from(transaction.instructions[0]);
  expect(systemInstruction.fromPublicKey).toEqual(from.publicKey);
  expect(systemInstruction.toPublicKey).toEqual(to.publicKey);
  expect(systemInstruction.amount).toEqual(amount);
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);
});

test('non-SystemInstruction error', () => {
  const from = new Account();
  const program = new Account();
  const to = new Account();

  const badProgramId = {
    keys: [
      {pubkey: from.publicKey, isSigner: true, isWritable: true},
      {pubkey: to.publicKey, isSigner: false, isWritable: true},
    ],
    programId: BudgetProgram.programId,
    data: Buffer.from([2, 0, 0, 0]),
  };
  expect(() => {
    new SystemInstruction(badProgramId);
  }).toThrow();

  const amount = 123;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const budgetPay = BudgetProgram.pay(
    from.publicKey,
    program.publicKey,
    to.publicKey,
    amount,
  );
  const transaction = new Transaction({recentBlockhash}).add(budgetPay);
  transaction.sign(from);

  expect(() => {
    SystemInstruction.from(transaction.instructions[1]);
  }).toThrow();

  transaction.instructions[0].data[0] = 4;
  expect(() => {
    SystemInstruction.from(transaction.instructions[0]);
  }).toThrow();
});
