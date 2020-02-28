// @flow

import {
  Account,
  BudgetProgram,
  Connection,
  SystemInstruction,
  SystemProgram,
  Transaction,
  sendAndConfirmRecentTransaction,
  LAMPORTS_PER_SOL,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {sleep} from '../src/util/sleep';
import {url} from './url';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

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

test('createNonceAccount', () => {
  const from = new Account();
  const nonceAccount = new Account();

  const transaction = SystemProgram.createNonceAccount(
    from.publicKey,
    nonceAccount.publicKey,
    from.publicKey,
    123,
  );

  expect(transaction.instructions).toHaveLength(2);
  expect(transaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(transaction.instructions[1].programId).toEqual(
    SystemProgram.programId,
  );
  // TODO: Validate transaction contents more
});

test('nonceWithdraw', () => {
  const from = new Account();
  const nonceAccount = new Account();
  const to = new Account();

  const transaction = SystemProgram.nonceWithdraw(
    nonceAccount.publicKey,
    from.publicKey,
    to.publicKey,
    123,
  );

  expect(transaction.keys).toHaveLength(5);
  expect(transaction.programId).toEqual(SystemProgram.programId);
  // TODO: Validate transaction contents more
});

test('nonceAuthorize', () => {
  const nonceAccount = new Account();
  const authorized = new Account();
  const newAuthorized = new Account();

  const transaction = SystemProgram.nonceAuthorize(
    nonceAccount.publicKey,
    authorized.publicKey,
    newAuthorized.publicKey,
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

test('SystemInstruction nonceWithdraw', () => {
  const nonceAccount = new Account();
  const authorized = new Account();
  const to = new Account();
  const amount = 42;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const nonceWithdraw = SystemProgram.nonceWithdraw(
    nonceAccount.publicKey,
    authorized.publicKey,
    to.publicKey,
    amount,
  );
  const transaction = new Transaction({recentBlockhash}).add(nonceWithdraw);

  const systemInstruction = SystemInstruction.from(transaction.instructions[0]);
  expect(systemInstruction.fromPublicKey).toEqual(nonceAccount.publicKey);
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
    new SystemInstruction(badProgramId, 'Create');
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

  transaction.instructions[0].data[0] = 11;
  expect(() => {
    SystemInstruction.from(transaction.instructions[0]);
  }).toThrow();
});

test('live Nonce actions', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');
  const nonceAccount = new Account();
  const from = new Account();
  const to = new Account();
  const authority = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);
  await connection.requestAirdrop(authority.publicKey, LAMPORTS_PER_SOL);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    SystemProgram.nonceSpace,
    'recent',
  );

  let createNonceAccount = SystemProgram.createNonceAccount(
    from.publicKey,
    nonceAccount.publicKey,
    from.publicKey,
    minimumAmount,
  );
  await sendAndConfirmRecentTransaction(
    connection,
    createNonceAccount,
    from,
    nonceAccount,
  );
  const nonceBalance = await connection.getBalance(nonceAccount.publicKey);
  expect(nonceBalance).toEqual(minimumAmount);

  const nonceQuery1 = await connection.getNonce(nonceAccount.publicKey);
  const nonceQuery2 = await connection.getNonce(nonceAccount.publicKey);
  expect(nonceQuery1.nonce).toEqual(nonceQuery2.nonce);

  await sleep(500);

  const advanceNonce = new Transaction().add(
    SystemProgram.nonceAdvance(nonceAccount.publicKey, from.publicKey),
  );
  await sendAndConfirmRecentTransaction(connection, advanceNonce, from);

  const nonceQuery3 = await connection.getNonce(nonceAccount.publicKey);
  expect(nonceQuery1.nonce).not.toEqual(nonceQuery3.nonce);
  const nonce = nonceQuery3.nonce;

  await sleep(500);

  let transfer = SystemProgram.transfer(
    from.publicKey,
    to.publicKey,
    minimumAmount,
  );
  transfer.nonceInfo = {
    nonce,
    nonceInstruction: SystemProgram.nonceAdvance(
      nonceAccount.publicKey,
      from.publicKey,
    ),
  };

  await sendAndConfirmRecentTransaction(connection, transfer, from);
  const toBalance = await connection.getBalance(to.publicKey);
  expect(toBalance).toEqual(minimumAmount);
});
