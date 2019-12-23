// @flow

import {
  Account,
  Authorized,
  Lockup,
  PublicKey,
  StakeAuthorizationLayout,
  StakeInstruction,
  StakeInstructionLayout,
  StakeProgram,
  SystemInstruction,
  SystemProgram,
  Transaction,
} from '../src';

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
    seed,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, from.publicKey),
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
    new Lockup(0, from.publicKey),
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

  expect(transaction.keys).toHaveLength(5);
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

  expect(transaction.keys).toHaveLength(2);
  expect(transaction.programId).toEqual(StakeProgram.programId);
  // TODO: Validate transaction contents more
});

test('redeemVoteCredits', () => {
  const stake = new Account();
  const vote = new Account();
  let transaction;

  transaction = StakeProgram.redeemVoteCredits(stake.publicKey, vote.publicKey);

  expect(transaction.keys).toHaveLength(5);
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
    seed,
    new Authorized(authorized.publicKey, authorized.publicKey),
    new Lockup(0, from.publicKey),
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
