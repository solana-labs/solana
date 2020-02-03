// @flow

import {
  Account,
  Authorized,
  Connection,
  Lockup,
  PublicKey,
  sendAndConfirmRecentTransaction,
  LAMPORTS_PER_SOL,
  StakeAuthorizationLayout,
  StakeInstruction,
  StakeInstructionLayout,
  StakeProgram,
  SystemInstruction,
  SystemProgram,
  Transaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

test('createAccountWithSeed', () => {
  const from = new Account();
  const seed = 'test string';
  const newAccountPubkey = PublicKey.createWithSeed(
    from.publicKey,
    seed,
    StakeProgram.programId,
  );
  const authorized = new Account();
  let transaction;

  transaction = StakeProgram.createAccountWithSeed(
    from.publicKey,
    newAccountPubkey,
    from.publicKey,
    seed,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, 0, from.publicKey),
    123,
  );

  expect(transaction.instructions).toHaveLength(2);
  expect(transaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(transaction.instructions[1].programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('createAccount', () => {
  const from = new Account();
  const newAccount = new Account();
  const authorized = new Account();
  let transaction;

  transaction = StakeProgram.createAccount(
    from.publicKey,
    newAccount.publicKey,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, 0, from.publicKey),
    123,
  );

  expect(transaction.instructions).toHaveLength(2);
  expect(transaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(transaction.instructions[1].programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('delegate', () => {
  const stake = new Account();
  const authorized = new Account();
  const vote = new Account();
  let transaction;

  transaction = StakeProgram.delegate(
    stake.publicKey,
    authorized.publicKey,
    vote.publicKey,
  );

  expect(transaction.keys).toHaveLength(6);
  expect(transaction.programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('authorize', () => {
  const stake = new Account();
  const authorized = new Account();
  const newAuthorized = new Account();
  const type = StakeAuthorizationLayout.Staker;
  let transaction;

  transaction = StakeProgram.authorize(
    stake.publicKey,
    authorized.publicKey,
    newAuthorized.publicKey,
    type,
  );

  expect(transaction.keys).toHaveLength(3);
  expect(transaction.programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('split', () => {
  const stake = new Account();
  const authorized = new Account();
  const newStake = new Account();
  let transaction;

  transaction = StakeProgram.split(
    stake.publicKey,
    authorized.publicKey,
    123,
    newStake.publicKey,
  );

  expect(transaction.instructions).toHaveLength(2);
  expect(transaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(transaction.instructions[1].programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('withdraw', () => {
  const stake = new Account();
  const withdrawer = new Account();
  const to = new Account();
  let transaction;

  transaction = StakeProgram.withdraw(
    stake.publicKey,
    withdrawer.publicKey,
    to.publicKey,
    123,
  );

  expect(transaction.keys).toHaveLength(5);
  expect(transaction.programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('deactivate', () => {
  const stake = new Account();
  const authorized = new Account();
  let transaction;

  transaction = StakeProgram.deactivate(stake.publicKey, authorized.publicKey);

  expect(transaction.keys).toHaveLength(3);
  expect(transaction.programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('StakeInstructions', () => {
  const from = new Account();
  const seed = 'test string';
  const newAccountPubkey = PublicKey.createWithSeed(
    from.publicKey,
    seed,
    StakeProgram.programId,
  );
  const authorized = new Account();
  const amount = 123;
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const createWithSeed = StakeProgram.createAccountWithSeed(
    from.publicKey,
    newAccountPubkey,
    from.publicKey,
    seed,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, 0, from.publicKey),
    amount,
  );
  const createWithSeedTransaction = new Transaction({recentBlockhash}).add(
    createWithSeed,
  );

  const systemInstruction = SystemInstruction.from(
    createWithSeedTransaction.instructions[0],
  );
  expect(systemInstruction.fromPublicKey).toEqual(from.publicKey);
  expect(systemInstruction.toPublicKey).toEqual(newAccountPubkey);
  expect(systemInstruction.amount).toEqual(amount);
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);

  const stakeInstruction = StakeInstruction.from(
    createWithSeedTransaction.instructions[1],
  );
  expect(stakeInstruction.type).toEqual(StakeInstructionLayout.Initialize);

  expect(() => {
    StakeInstruction.from(createWithSeedTransaction.instructions[0]);
  }).toThrow();

  const stake = new Account();
  const vote = new Account();
  const delegate = StakeProgram.delegate(
    stake.publicKey,
    authorized.publicKey,
    vote.publicKey,
  );

  const delegateTransaction = new Transaction({recentBlockhash}).add(delegate);

  const anotherStakeInstruction = StakeInstruction.from(
    delegateTransaction.instructions[0],
  );
  expect(anotherStakeInstruction.type).toEqual(
    StakeInstructionLayout.DelegateStake,
  );
});

test('live staking actions', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');
  const voteAccounts = await connection.getVoteAccounts();
  const voteAccount = voteAccounts.current.concat(voteAccounts.delinquent)[0];
  const votePubkey = new PublicKey(voteAccount.votePubkey);

  const from = new Account();
  const authorized = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);
  await connection.requestAirdrop(authorized.publicKey, 2 * LAMPORTS_PER_SOL);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    StakeProgram.space,
    'recent',
  );

  // Create Stake account with seed
  const seed = 'test string';
  const newAccountPubkey = PublicKey.createWithSeed(
    from.publicKey,
    seed,
    StakeProgram.programId,
  );

  let createAndInitializeWithSeed = StakeProgram.createAccountWithSeed(
    from.publicKey,
    newAccountPubkey,
    from.publicKey,
    seed,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, 0, new PublicKey('0x00')),
    3 * minimumAmount + 42,
  );

  await sendAndConfirmRecentTransaction(
    connection,
    createAndInitializeWithSeed,
    from,
  );
  let originalStakeBalance = await connection.getBalance(newAccountPubkey);
  expect(originalStakeBalance).toEqual(3 * minimumAmount + 42);

  let delegation = StakeProgram.delegate(
    newAccountPubkey,
    authorized.publicKey,
    votePubkey,
  );
  await sendAndConfirmRecentTransaction(connection, delegation, authorized);

  // Test that withdraw fails before deactivation
  const recipient = new Account();
  let withdraw = StakeProgram.withdraw(
    newAccountPubkey,
    authorized.publicKey,
    recipient.publicKey,
    1000,
  );
  await expect(
    sendAndConfirmRecentTransaction(connection, withdraw, authorized),
  ).rejects.toThrow();

  // Split stake
  const newStake = new Account();
  let split = StakeProgram.split(
    newAccountPubkey,
    authorized.publicKey,
    minimumAmount + 20,
    newStake.publicKey,
  );
  await sendAndConfirmRecentTransaction(
    connection,
    split,
    authorized,
    newStake,
  );

  // Authorize to new account
  const newAuthorized = new Account();
  await connection.requestAirdrop(newAuthorized.publicKey, LAMPORTS_PER_SOL);

  let authorize = StakeProgram.authorize(
    newAccountPubkey,
    authorized.publicKey,
    newAuthorized.publicKey,
    StakeAuthorizationLayout.Withdrawer,
  );
  await sendAndConfirmRecentTransaction(connection, authorize, authorized);
  authorize = StakeProgram.authorize(
    newAccountPubkey,
    authorized.publicKey,
    newAuthorized.publicKey,
    StakeAuthorizationLayout.Staker,
  );
  await sendAndConfirmRecentTransaction(connection, authorize, authorized);

  // Test old authorized can't deactivate
  let deactivateNotAuthorized = StakeProgram.deactivate(
    newAccountPubkey,
    authorized.publicKey,
  );
  await expect(
    sendAndConfirmRecentTransaction(
      connection,
      deactivateNotAuthorized,
      authorized,
    ),
  ).rejects.toThrow();

  // Deactivate stake
  let deactivate = StakeProgram.deactivate(
    newAccountPubkey,
    newAuthorized.publicKey,
  );
  await sendAndConfirmRecentTransaction(connection, deactivate, newAuthorized);

  // Test that withdraw succeeds after deactivation
  withdraw = StakeProgram.withdraw(
    newAccountPubkey,
    newAuthorized.publicKey,
    recipient.publicKey,
    minimumAmount + 20,
  );
  await sendAndConfirmRecentTransaction(connection, withdraw, newAuthorized);
  const balance = await connection.getBalance(newAccountPubkey);
  expect(balance).toEqual(minimumAmount + 2);
  const recipientBalance = await connection.getBalance(recipient.publicKey);
  expect(recipientBalance).toEqual(minimumAmount + 20);
});
