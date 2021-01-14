// @flow

import {
  Account,
  Connection,
  PublicKey,
  StakeProgram,
  SystemInstruction,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
} from '../src';
import {NONCE_ACCOUNT_LENGTH} from '../src/nonce-account';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {newAccountWithLamports} from './new-account-with-lamports';
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
    space: 0,
    programId: SystemProgram.programId,
  };
  const transaction = new Transaction().add(
    SystemProgram.createAccount(params),
  );
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
  const transaction = new Transaction().add(SystemProgram.transfer(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeTransfer(systemInstruction));
});

test('transferWithSeed', () => {
  const params = {
    fromPubkey: new Account().publicKey,
    basePubkey: new Account().publicKey,
    toPubkey: new Account().publicKey,
    lamports: 123,
    seed: '你好',
    programId: new Account().publicKey,
  };
  const transaction = new Transaction().add(SystemProgram.transfer(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(
    SystemInstruction.decodeTransferWithSeed(systemInstruction),
  );
});

test('allocate', () => {
  const params = {
    accountPubkey: new Account().publicKey,
    space: 42,
  };
  const transaction = new Transaction().add(SystemProgram.allocate(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeAllocate(systemInstruction));
});

test('allocateWithSeed', () => {
  const params = {
    accountPubkey: new Account().publicKey,
    basePubkey: new Account().publicKey,
    seed: '你好',
    space: 42,
    programId: new Account().publicKey,
  };
  const transaction = new Transaction().add(SystemProgram.allocate(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(
    SystemInstruction.decodeAllocateWithSeed(systemInstruction),
  );
});

test('assign', () => {
  const params = {
    accountPubkey: new Account().publicKey,
    programId: new Account().publicKey,
  };
  const transaction = new Transaction().add(SystemProgram.assign(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeAssign(systemInstruction));
});

test('assignWithSeed', () => {
  const params = {
    accountPubkey: new Account().publicKey,
    basePubkey: new Account().publicKey,
    seed: '你好',
    programId: new Account().publicKey,
  };
  const transaction = new Transaction().add(SystemProgram.assign(params));
  expect(transaction.instructions).toHaveLength(1);
  const [systemInstruction] = transaction.instructions;
  expect(params).toEqual(
    SystemInstruction.decodeAssignWithSeed(systemInstruction),
  );
});

test('createAccountWithSeed', () => {
  const fromPubkey = new Account().publicKey;
  const params = {
    fromPubkey,
    newAccountPubkey: new Account().publicKey,
    basePubkey: fromPubkey,
    seed: 'hi there',
    lamports: 123,
    space: 0,
    programId: SystemProgram.programId,
  };
  const transaction = new Transaction().add(
    SystemProgram.createAccountWithSeed(params),
  );
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

  const transaction = new Transaction().add(
    SystemProgram.createNonceAccount(params),
  );
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

test('createNonceAccount with seed', () => {
  const fromPubkey = new Account().publicKey;
  const params = {
    fromPubkey,
    noncePubkey: new Account().publicKey,
    authorizedPubkey: fromPubkey,
    basePubkey: fromPubkey,
    seed: 'hi there',
    lamports: 123,
  };

  const transaction = new Transaction().add(
    SystemProgram.createNonceAccount(params),
  );
  expect(transaction.instructions).toHaveLength(2);
  const [createInstruction, initInstruction] = transaction.instructions;

  const createParams = {
    fromPubkey: params.fromPubkey,
    newAccountPubkey: params.noncePubkey,
    basePubkey: fromPubkey,
    seed: 'hi there',
    lamports: params.lamports,
    space: NONCE_ACCOUNT_LENGTH,
    programId: SystemProgram.programId,
  };
  expect(createParams).toEqual(
    SystemInstruction.decodeCreateWithSeed(createInstruction),
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
  const transaction = new Transaction().add(
    SystemProgram.nonceWithdraw(params),
  );
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

  const transaction = new Transaction().add(
    SystemProgram.nonceAuthorize(params),
  );
  expect(transaction.instructions).toHaveLength(1);
  const [instruction] = transaction.instructions;
  expect(params).toEqual(SystemInstruction.decodeNonceAuthorize(instruction));
});

test('non-SystemInstruction error', () => {
  const from = new Account();
  const to = new Account();

  const badProgramId = {
    keys: [
      {pubkey: from.publicKey, isSigner: true, isWritable: true},
      {pubkey: to.publicKey, isSigner: false, isWritable: true},
    ],
    programId: StakeProgram.programId,
    data: Buffer.from([2, 0, 0, 0]),
  };
  expect(() => {
    SystemInstruction.decodeInstructionType(
      new TransactionInstruction(badProgramId),
    );
  }).toThrow();

  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const params = {stakePubkey, authorizedPubkey};
  const transaction = StakeProgram.deactivate(params);

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

  const connection = new Connection(url, 'singleGossip');
  const nonceAccount = new Account();
  const from = await newAccountWithLamports(connection, 2 * LAMPORTS_PER_SOL);
  const to = new Account();
  const newAuthority = await newAccountWithLamports(
    connection,
    LAMPORTS_PER_SOL,
  );

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    NONCE_ACCOUNT_LENGTH,
  );

  let createNonceAccount = new Transaction().add(
    SystemProgram.createNonceAccount({
      fromPubkey: from.publicKey,
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: from.publicKey,
      lamports: minimumAmount,
    }),
  );
  await sendAndConfirmTransaction(
    connection,
    createNonceAccount,
    [from, nonceAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  const nonceBalance = await connection.getBalance(nonceAccount.publicKey);
  expect(nonceBalance).toEqual(minimumAmount);

  const nonceQuery1 = await connection.getNonce(nonceAccount.publicKey);
  if (nonceQuery1 === null) {
    expect(nonceQuery1).not.toBeNull();
    return;
  }

  const nonceQuery2 = await connection.getNonce(nonceAccount.publicKey);
  if (nonceQuery2 === null) {
    expect(nonceQuery2).not.toBeNull();
    return;
  }

  expect(nonceQuery1.nonce).toEqual(nonceQuery2.nonce);

  // Wait for blockhash to advance
  await sleep(500);

  const advanceNonce = new Transaction().add(
    SystemProgram.nonceAdvance({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: from.publicKey,
    }),
  );
  await sendAndConfirmTransaction(connection, advanceNonce, [from], {
    commitment: 'singleGossip',
    preflightCommitment: 'singleGossip',
  });
  const nonceQuery3 = await connection.getNonce(nonceAccount.publicKey);
  if (nonceQuery3 === null) {
    expect(nonceQuery3).not.toBeNull();
    return;
  }
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
  await sendAndConfirmTransaction(connection, authorizeNonce, [from], {
    commitment: 'singleGossip',
    preflightCommitment: 'singleGossip',
  });

  let transfer = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: from.publicKey,
      toPubkey: to.publicKey,
      lamports: minimumAmount,
    }),
  );
  transfer.nonceInfo = {
    nonce,
    nonceInstruction: SystemProgram.nonceAdvance({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: newAuthority.publicKey,
    }),
  };

  await sendAndConfirmTransaction(connection, transfer, [from, newAuthority], {
    commitment: 'singleGossip',
    preflightCommitment: 'singleGossip',
  });
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
  await sendAndConfirmTransaction(connection, withdrawNonce, [newAuthority], {
    commitment: 'singleGossip',
    preflightCommitment: 'singleGossip',
  });
  expect(await connection.getBalance(nonceAccount.publicKey)).toEqual(0);
  const withdrawBalance = await connection.getBalance(
    withdrawAccount.publicKey,
  );
  expect(withdrawBalance).toEqual(minimumAmount);
});

test('live withSeed actions', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'singleGossip');
  const baseAccount = await newAccountWithLamports(
    connection,
    2 * LAMPORTS_PER_SOL,
  );
  const basePubkey = baseAccount.publicKey;
  const seed = 'hi there';
  const programId = new Account().publicKey;
  const createAccountWithSeedAddress = await PublicKey.createWithSeed(
    basePubkey,
    seed,
    programId,
  );
  const space = 0;

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    space,
  );

  const createAccountWithSeedParams = {
    fromPubkey: basePubkey,
    newAccountPubkey: createAccountWithSeedAddress,
    basePubkey,
    seed,
    lamports: minimumAmount,
    space,
    programId,
  };
  const createAccountWithSeedTransaction = new Transaction().add(
    SystemProgram.createAccountWithSeed(createAccountWithSeedParams),
  );
  await sendAndConfirmTransaction(
    connection,
    createAccountWithSeedTransaction,
    [baseAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  const createAccountWithSeedBalance = await connection.getBalance(
    createAccountWithSeedAddress,
  );
  expect(createAccountWithSeedBalance).toEqual(minimumAmount);

  // Transfer to a derived address
  const programId2 = new Account().publicKey;
  const transferWithSeedAddress = await PublicKey.createWithSeed(
    basePubkey,
    seed,
    programId2,
  );
  await sendAndConfirmTransaction(
    connection,
    new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: baseAccount.publicKey,
        toPubkey: transferWithSeedAddress,
        lamports: 3 * minimumAmount,
      }),
    ),
    [baseAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  let transferWithSeedAddressBalance = await connection.getBalance(
    transferWithSeedAddress,
  );
  expect(transferWithSeedAddressBalance).toEqual(3 * minimumAmount);

  // Test TransferWithSeed
  const programId3 = new Account();
  const toPubkey = await PublicKey.createWithSeed(
    basePubkey,
    seed,
    programId3.publicKey,
  );
  const transferWithSeedParams = {
    fromPubkey: transferWithSeedAddress,
    basePubkey,
    toPubkey,
    lamports: 2 * minimumAmount,
    seed,
    programId: programId2,
  };
  const transferWithSeedTransaction = new Transaction().add(
    SystemProgram.transfer(transferWithSeedParams),
  );
  await sendAndConfirmTransaction(
    connection,
    transferWithSeedTransaction,
    [baseAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  const toBalance = await connection.getBalance(toPubkey);
  expect(toBalance).toEqual(2 * minimumAmount);
  transferWithSeedAddressBalance = await connection.getBalance(
    createAccountWithSeedAddress,
  );
  expect(transferWithSeedAddressBalance).toEqual(minimumAmount);

  // Test AllocateWithSeed
  const allocateWithSeedParams = {
    accountPubkey: toPubkey,
    basePubkey,
    seed,
    space: 10,
    programId: programId3.publicKey,
  };
  const allocateWithSeedTransaction = new Transaction().add(
    SystemProgram.allocate(allocateWithSeedParams),
  );
  await sendAndConfirmTransaction(
    connection,
    allocateWithSeedTransaction,
    [baseAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  let account = await connection.getAccountInfo(toPubkey);
  if (account === null) {
    expect(account).not.toBeNull();
    return;
  }
  expect(account.data).toHaveLength(10);

  // Test AssignWithSeed
  const assignWithSeedParams = {
    accountPubkey: toPubkey,
    basePubkey,
    seed,
    programId: programId3.publicKey,
  };
  const assignWithSeedTransaction = new Transaction().add(
    SystemProgram.assign(assignWithSeedParams),
  );
  await sendAndConfirmTransaction(
    connection,
    assignWithSeedTransaction,
    [baseAccount],
    {commitment: 'singleGossip', preflightCommitment: 'singleGossip'},
  );
  account = await connection.getAccountInfo(toPubkey);
  if (account === null) {
    expect(account).not.toBeNull();
    return;
  }
  expect(account.owner).toEqual(programId3.publicKey);
});
