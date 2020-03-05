// @flow

import {
  Account,
  BudgetProgram,
  Connection,
  SystemInstruction,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmRecentTransaction,
  LAMPORTS_PER_SOL,
} from '../src';
import {NONCE_ACCOUNT_LENGTH} from '../src/nonce-account';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {sleep} from '../src/util/sleep';
import {url} from './url';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

test('createAccount', () => {
  const params = {
    fromPubkey: new Account().publicKey,
    newAccountPubkey: new Account().publicKey,
    lamports: 123,
    space: BudgetProgram.space,
    programId: BudgetProgram.programId,
  };
  const transaction = SystemProgram.createAccount(params);
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(
    SystemInstruction.decodeCreateAccount(systemInstruction),
  );
});

test('transfer', () => {
  const params = {
    fromPubkey: new Account().publicKey,
    toPubkey: new Account().publicKey,
    lamports: 123,
  };
  const transaction = SystemProgram.transfer(params);
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeTransfer(systemInstruction));
});

test('assign', () => {
  const params = {
    fromPubkey: new Account().publicKey,
    programId: new Account().publicKey,
  };
  const transaction = SystemProgram.assign(params);
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeAssign(systemInstruction));
});

test('createAccountWithSeed', () => {
  const fromPubkey = new Account().publicKey;
  const params = {
    fromPubkey,
    newAccountPubkey: new Account().publicKey,
    basePubkey: fromPubkey,
    seed: 'hi there',
    lamports: 123,
    space: BudgetProgram.space,
    programId: BudgetProgram.programId,
  };
  const transaction = SystemProgram.createAccountWithSeed(params);
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(
    SystemInstruction.decodeCreateWithSeed(systemInstruction),
  );
});

test('createNonceAccount', () => {
  const fromPubkey = new Account().publicKey;
  const params = {
    fromPubkey,
    noncePubkey: new Account().publicKey,
    authorizedPubkey: fromPubkey,
    lamports: 123,
  };

  const transaction = SystemProgram.createNonceAccount(params);
  expect(transaction.instructions).toHaveLength(2);
  const [createInstruction, initInstruction] = transaction.instructions;

  const createParams = {
    fromPubkey: params.fromPubkey,
    newAccountPubkey: params.noncePubkey,
    lamports: params.lamports,
    space: NONCE_ACCOUNT_LENGTH,
    programId: SystemProgram.programId,
  };
  expect(createParams).toEqual(
    SystemInstruction.decodeCreateAccount(createInstruction),
  );

  const initParams = {
    noncePubkey: params.noncePubkey,
    authorizedPubkey: fromPubkey,
  };
  expect(initParams).toEqual(
    SystemInstruction.decodeNonceInitialize(initInstruction),
  );
});

test('nonceAdvance', () => {
  const params = {
    noncePubkey: new Account().publicKey,
    authorizedPubkey: new Account().publicKey,
  };
  const instruction = SystemProgram.nonceAdvance(params);
  expect(params).toEqual(SystemInstruction.decodeNonceAdvance(instruction));
});

test('nonceWithdraw', () => {
  const params = {
    noncePubkey: new Account().publicKey,
    authorizedPubkey: new Account().publicKey,
    toPubkey: new Account().publicKey,
    lamports: 123,
  };
  const transaction = SystemProgram.nonceWithdraw(params);
  expect(transaction.instructions).toHaveLength(1);
  const [instruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeNonceWithdraw(instruction));
});

test('nonceAuthorize', () => {
  const params = {
    noncePubkey: new Account().publicKey,
    authorizedPubkey: new Account().publicKey,
    newAuthorizedPubkey: new Account().publicKey,
  };

  const transaction = SystemProgram.nonceAuthorize(params);
  expect(transaction.instructions).toHaveLength(1);
  const [instruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeNonceAuthorize(instruction));
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
    SystemInstruction.decodeInstructionType(
      new TransactionInstruction(badProgramId),
    );
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
    SystemInstruction.decodeInstructionType(transaction.instructions[1]);
  }).toThrow();

  transaction.instructions[0].data[0] = 11;
  expect(() => {
    SystemInstruction.decodeInstructionType(transaction.instructions[0]);
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
  const newAuthority = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);
  await connection.requestAirdrop(authority.publicKey, LAMPORTS_PER_SOL);
  await connection.requestAirdrop(newAuthority.publicKey, LAMPORTS_PER_SOL);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    NONCE_ACCOUNT_LENGTH,
    'recent',
  );

  let createNonceAccount = SystemProgram.createNonceAccount({
    fromPubkey: from.publicKey,
    noncePubkey: nonceAccount.publicKey,
    authorizedPubkey: from.publicKey,
    lamports: minimumAmount,
  });
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

  // Wait for blockhash to advance
  await sleep(500);

  const advanceNonce = new Transaction().add(
    SystemProgram.nonceAdvance({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: from.publicKey,
    }),
  );
  await sendAndConfirmRecentTransaction(connection, advanceNonce, from);
  const nonceQuery3 = await connection.getNonce(nonceAccount.publicKey);
  expect(nonceQuery1.nonce).not.toEqual(nonceQuery3.nonce);
  const nonce = nonceQuery3.nonce;

  // Wait for blockhash to advance
  await sleep(500);

  const authorizeNonce = new Transaction().add(
    SystemProgram.nonceAuthorize({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: from.publicKey,
      newAuthorizedPubkey: newAuthority.publicKey,
    }),
  );
  await sendAndConfirmRecentTransaction(connection, authorizeNonce, from);

  let transfer = SystemProgram.transfer({
    fromPubkey: from.publicKey,
    toPubkey: to.publicKey,
    lamports: minimumAmount,
  });
  transfer.nonceInfo = {
    nonce,
    nonceInstruction: SystemProgram.nonceAdvance({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: newAuthority.publicKey,
    }),
  };

  await sendAndConfirmRecentTransaction(
    connection,
    transfer,
    from,
    newAuthority,
  );
  const toBalance = await connection.getBalance(to.publicKey);
  expect(toBalance).toEqual(minimumAmount);

  // Wait for blockhash to advance
  await sleep(500);

  const withdrawAccount = new Account();
  const withdrawNonce = new Transaction().add(
    SystemProgram.nonceWithdraw({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: newAuthority.publicKey,
      lamports: minimumAmount,
      toPubkey: withdrawAccount.publicKey,
    }),
  );
  await sendAndConfirmRecentTransaction(
    connection,
    withdrawNonce,
    newAuthority,
  );
  expect(await connection.getBalance(nonceAccount.publicKey)).toEqual(0);
  const withdrawBalance = await connection.getBalance(
    withdrawAccount.publicKey,
  );
  expect(withdrawBalance).toEqual(minimumAmount);
});
