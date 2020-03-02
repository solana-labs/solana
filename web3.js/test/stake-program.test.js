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
  const fromPubkey = new Account().publicKey;
  const seed = 'test string';
  const newAccountPubkey = PublicKey.createWithSeed(
    fromPubkey,
    seed,
    StakeProgram.programId,
  );
  const authorizedPubkey = new Account().publicKey;
  const authorized = new Authorized(authorizedPubkey, authorizedPubkey);
  const lockup = new Lockup(0, 0, fromPubkey);
  const transaction = StakeProgram.createAccountWithSeed({
    fromPubkey,
    stakePubkey: newAccountPubkey,
    basePubkey: fromPubkey,
    seed,
    authorized,
    lockup,
    lamports: 123,
  });

  expect(transaction.instructions).toHaveLength(2);
  const [systemInstruction, stakeInstruction] = transaction.instructions;
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);

  // TODO decode system instruction

  const params = StakeInstruction.decodeInitialize(stakeInstruction);
  expect(params.stakePubkey).toEqual(newAccountPubkey);
  expect(params.authorized).toEqual(authorized);
  expect(params.lockup).toEqual(lockup);
});

test('createAccount', () => {
  const fromPubkey = new Account().publicKey;
  const newAccountPubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const authorized = new Authorized(authorizedPubkey, authorizedPubkey);
  const lockup = new Lockup(0, 0, fromPubkey);
  const transaction = StakeProgram.createAccount({
    fromPubkey,
    stakePubkey: newAccountPubkey,
    authorized,
    lockup,
    lamports: 123,
  });

  expect(transaction.instructions).toHaveLength(2);
  const [systemInstruction, stakeInstruction] = transaction.instructions;
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);

  // TODO decode system instruction

  const params = StakeInstruction.decodeInitialize(stakeInstruction);
  expect(params.stakePubkey).toEqual(newAccountPubkey);
  expect(params.authorized).toEqual(authorized);
  expect(params.lockup).toEqual(lockup);
});

test('delegate', () => {
  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const votePubkey = new Account().publicKey;
  const params = {
    stakePubkey,
    authorizedPubkey,
    votePubkey,
  };
  const transaction = StakeProgram.delegate(params);
  expect(transaction.instructions).toHaveLength(1);
  const [stakeInstruction] = transaction.instructions;
  expect(params).toEqual(StakeInstruction.decodeDelegate(stakeInstruction));
});

test('authorize', () => {
  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const newAuthorizedPubkey = new Account().publicKey;
  const stakeAuthorizationType = StakeAuthorizationLayout.Staker;
  const params = {
    stakePubkey,
    authorizedPubkey,
    newAuthorizedPubkey,
    stakeAuthorizationType,
  };
  const transaction = StakeProgram.authorize(params);
  expect(transaction.instructions).toHaveLength(1);
  const [stakeInstruction] = transaction.instructions;
  expect(params).toEqual(StakeInstruction.decodeAuthorize(stakeInstruction));
});

test('split', () => {
  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const splitStakePubkey = new Account().publicKey;
  const params = {
    stakePubkey,
    authorizedPubkey,
    splitStakePubkey,
    lamports: 123,
  };
  const transaction = StakeProgram.split(params);
  expect(transaction.instructions).toHaveLength(2);
  const [systemInstruction, stakeInstruction] = transaction.instructions;
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);
  expect(params).toEqual(StakeInstruction.decodeSplit(stakeInstruction));
});

test('withdraw', () => {
  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const toPubkey = new Account().publicKey;
  const params = {
    stakePubkey,
    authorizedPubkey,
    toPubkey,
    lamports: 123,
  };
  const transaction = StakeProgram.withdraw(params);
  expect(transaction.instructions).toHaveLength(1);
  const [stakeInstruction] = transaction.instructions;
  expect(params).toEqual(StakeInstruction.decodeWithdraw(stakeInstruction));
});

test('deactivate', () => {
  const stakePubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const params = {stakePubkey, authorizedPubkey};
  const transaction = StakeProgram.deactivate(params);
  expect(transaction.instructions).toHaveLength(1);
  const [stakeInstruction] = transaction.instructions;
  expect(params).toEqual(StakeInstruction.decodeDeactivate(stakeInstruction));
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
  const createWithSeed = StakeProgram.createAccountWithSeed({
    fromPubkey: from.publicKey,
    stakePubkey: newAccountPubkey,
    basePubkey: from.publicKey,
    seed,
    authorized: new Authorized(authorized.publicKey, authorized.publicKey),
    lockup: new Lockup(0, 0, from.publicKey),
    lamports: amount,
  });
  const createWithSeedTransaction = new Transaction({recentBlockhash}).add(
    createWithSeed,
  );

  expect(createWithSeedTransaction.instructions).toHaveLength(2);
  const systemInstruction = SystemInstruction.from(
    createWithSeedTransaction.instructions[0],
  );
  expect(systemInstruction.fromPublicKey).toEqual(from.publicKey);
  expect(systemInstruction.toPublicKey).toEqual(newAccountPubkey);
  expect(systemInstruction.amount).toEqual(amount);
  expect(systemInstruction.programId).toEqual(SystemProgram.programId);

  const stakeInstructionType = StakeInstruction.decodeInstructionType(
    createWithSeedTransaction.instructions[1],
  );
  expect(stakeInstructionType).toEqual('Initialize');

  expect(() => {
    StakeInstruction.decodeInstructionType(
      createWithSeedTransaction.instructions[0],
    );
  }).toThrow();

  const stake = new Account();
  const vote = new Account();
  const delegate = StakeProgram.delegate({
    stakePubkey: stake.publicKey,
    authorizedPubkey: authorized.publicKey,
    votePubkey: vote.publicKey,
  });

  const delegateTransaction = new Transaction({recentBlockhash}).add(delegate);
  const anotherStakeInstructionType = StakeInstruction.decodeInstructionType(
    delegateTransaction.instructions[0],
  );
  expect(anotherStakeInstructionType).toEqual('Delegate');
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

  let createAndInitializeWithSeed = StakeProgram.createAccountWithSeed({
    fromPubkey: from.publicKey,
    stakePubkey: newAccountPubkey,
    basePubkey: from.publicKey,
    seed,
    authorized: new Authorized(authorized.publicKey, authorized.publicKey),
    lockup: new Lockup(0, 0, new PublicKey('0x00')),
    lamports: 3 * minimumAmount + 42,
  });

  await sendAndConfirmRecentTransaction(
    connection,
    createAndInitializeWithSeed,
    from,
  );
  let originalStakeBalance = await connection.getBalance(newAccountPubkey);
  expect(originalStakeBalance).toEqual(3 * minimumAmount + 42);

  let delegation = StakeProgram.delegate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    votePubkey,
  });
  await sendAndConfirmRecentTransaction(connection, delegation, authorized);

  // Test that withdraw fails before deactivation
  const recipient = new Account();
  let withdraw = StakeProgram.withdraw({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    toPubkey: recipient.publicKey,
    lamports: 1000,
  });
  await expect(
    sendAndConfirmRecentTransaction(connection, withdraw, authorized),
  ).rejects.toThrow();

  // Split stake
  const newStake = new Account();
  let split = StakeProgram.split({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    splitStakePubkey: newStake.publicKey,
    lamports: minimumAmount + 20,
  });
  await sendAndConfirmRecentTransaction(
    connection,
    split,
    authorized,
    newStake,
  );

  // Authorize to new account
  const newAuthorized = new Account();
  await connection.requestAirdrop(newAuthorized.publicKey, LAMPORTS_PER_SOL);

  let authorize = StakeProgram.authorize({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    newAuthorizedPubkey: newAuthorized.publicKey,
    stakeAuthorizationType: StakeAuthorizationLayout.Withdrawer,
  });
  await sendAndConfirmRecentTransaction(connection, authorize, authorized);
  authorize = StakeProgram.authorize({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    newAuthorizedPubkey: newAuthorized.publicKey,
    stakeAuthorizationType: StakeAuthorizationLayout.Staker,
  });
  await sendAndConfirmRecentTransaction(connection, authorize, authorized);

  // Test old authorized can't deactivate
  let deactivateNotAuthorized = StakeProgram.deactivate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
  });
  await expect(
    sendAndConfirmRecentTransaction(
      connection,
      deactivateNotAuthorized,
      authorized,
    ),
  ).rejects.toThrow();

  // Deactivate stake
  let deactivate = StakeProgram.deactivate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: newAuthorized.publicKey,
  });
  await sendAndConfirmRecentTransaction(connection, deactivate, newAuthorized);

  // Test that withdraw succeeds after deactivation
  withdraw = StakeProgram.withdraw({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: newAuthorized.publicKey,
    toPubkey: recipient.publicKey,
    lamports: minimumAmount + 20,
  });
  await sendAndConfirmRecentTransaction(connection, withdraw, newAuthorized);
  const balance = await connection.getBalance(newAccountPubkey);
  expect(balance).toEqual(minimumAmount + 2);
  const recipientBalance = await connection.getBalance(recipient.publicKey);
  expect(recipientBalance).toEqual(minimumAmount + 20);
});
