// @flow

import {
  Account,
  Authorized,
  Connection,
  Lockup,
  PublicKey,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  StakeAuthorizationLayout,
  StakeInstruction,
  StakeProgram,
  SystemInstruction,
  Transaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

test('createAccountWithSeed', async () => {
  const fromPubkey = new Account().publicKey;
  const seed = 'test string';
  const newAccountPubkey = await PublicKey.createWithSeed(
    fromPubkey,
    seed,
    StakeProgram.programId,
  );
  const authorizedPubkey = new Account().publicKey;
  const authorized = new Authorized(authorizedPubkey, authorizedPubkey);
  const lockup = new Lockup(0, 0, fromPubkey);
  const lamports = 123;
  const transaction = StakeProgram.createAccountWithSeed({
    fromPubkey,
    stakePubkey: newAccountPubkey,
    basePubkey: fromPubkey,
    seed,
    authorized,
    lockup,
    lamports,
  });
  expect(transaction.instructions).toHaveLength(2);
  const [systemInstruction, stakeInstruction] = transaction.instructions;
  const systemParams = {
    fromPubkey,
    newAccountPubkey,
    basePubkey: fromPubkey,
    seed,
    lamports,
    space: StakeProgram.space,
    programId: StakeProgram.programId,
  };
  expect(systemParams).toEqual(
    SystemInstruction.decodeCreateWithSeed(systemInstruction),
  );
  const initParams = {stakePubkey: newAccountPubkey, authorized, lockup};
  expect(initParams).toEqual(
    StakeInstruction.decodeInitialize(stakeInstruction),
  );
});

test('createAccount', () => {
  const fromPubkey = new Account().publicKey;
  const newAccountPubkey = new Account().publicKey;
  const authorizedPubkey = new Account().publicKey;
  const authorized = new Authorized(authorizedPubkey, authorizedPubkey);
  const lockup = new Lockup(0, 0, fromPubkey);
  const lamports = 123;
  const transaction = StakeProgram.createAccount({
    fromPubkey,
    stakePubkey: newAccountPubkey,
    authorized,
    lockup,
    lamports,
  });
  expect(transaction.instructions).toHaveLength(2);
  const [systemInstruction, stakeInstruction] = transaction.instructions;
  const systemParams = {
    fromPubkey,
    newAccountPubkey,
    lamports,
    space: StakeProgram.space,
    programId: StakeProgram.programId,
  };
  expect(systemParams).toEqual(
    SystemInstruction.decodeCreateAccount(systemInstruction),
  );

  const initParams = {stakePubkey: newAccountPubkey, authorized, lockup};
  expect(initParams).toEqual(
    StakeInstruction.decodeInitialize(stakeInstruction),
  );
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

test('authorizeWithSeed', () => {
  const stakePubkey = new Account().publicKey;
  const authorityBase = new Account().publicKey;
  const authoritySeed = 'test string';
  const authorityOwner = new Account().publicKey;
  const newAuthorizedPubkey = new Account().publicKey;
  const stakeAuthorizationType = StakeAuthorizationLayout.Staker;
  const params = {
    stakePubkey,
    authorityBase,
    authoritySeed,
    authorityOwner,
    newAuthorizedPubkey,
    stakeAuthorizationType,
  };
  const transaction = StakeProgram.authorizeWithSeed(params);
  expect(transaction.instructions).toHaveLength(1);
  const [stakeInstruction] = transaction.instructions;
  expect(params).toEqual(StakeInstruction.decodeAuthorizeWithSeed(stakeInstruction));
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
  const systemParams = {
    fromPubkey: authorizedPubkey,
    newAccountPubkey: splitStakePubkey,
    lamports: 0,
    space: StakeProgram.space,
    programId: StakeProgram.programId,
  };
  expect(systemParams).toEqual(
    SystemInstruction.decodeCreateAccount(systemInstruction),
  );
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

test('StakeInstructions', async () => {
  const from = new Account();
  const seed = 'test string';
  const newAccountPubkey = await PublicKey.createWithSeed(
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
  const systemInstructionType = SystemInstruction.decodeInstructionType(
    createWithSeedTransaction.instructions[0],
  );
  expect(systemInstructionType).toEqual('CreateWithSeed');

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

  {
    // Create Stake account without seed
    const newStakeAccount = new Account();
    let createAndInitialize = StakeProgram.createAccount({
      fromPubkey: from.publicKey,
      stakePubkey: newStakeAccount.publicKey,
      authorized: new Authorized(authorized.publicKey, authorized.publicKey),
      lockup: new Lockup(0, 0, new PublicKey(0)),
      lamports: minimumAmount + 42,
    });

    await sendAndConfirmTransaction(
      connection,
      createAndInitialize,
      [from, newStakeAccount],
      {confirmations: 0, skipPreflight: true},
    );
    expect(await connection.getBalance(newStakeAccount.publicKey)).toEqual(
      minimumAmount + 42,
    );

    let delegation = StakeProgram.delegate({
      stakePubkey: newStakeAccount.publicKey,
      authorizedPubkey: authorized.publicKey,
      votePubkey,
    });
    await sendAndConfirmTransaction(connection, delegation, [authorized], {
      confirmations: 0,
      skipPreflight: true,
    });
  }

  // Create Stake account with seed
  const seed = 'test string';
  const newAccountPubkey = await PublicKey.createWithSeed(
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
    lockup: new Lockup(0, 0, new PublicKey(0)),
    lamports: 3 * minimumAmount + 42,
  });

  await sendAndConfirmTransaction(
    connection,
    createAndInitializeWithSeed,
    [from],
    {confirmations: 0, skipPreflight: true},
  );
  let originalStakeBalance = await connection.getBalance(newAccountPubkey);
  expect(originalStakeBalance).toEqual(3 * minimumAmount + 42);

  let delegation = StakeProgram.delegate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    votePubkey,
  });
  await sendAndConfirmTransaction(connection, delegation, [authorized], {
    confirmations: 0,
    skipPreflight: true,
  });

  // Test that withdraw fails before deactivation
  const recipient = new Account();
  let withdraw = StakeProgram.withdraw({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    toPubkey: recipient.publicKey,
    lamports: 1000,
  });
  await expect(
    sendAndConfirmTransaction(connection, withdraw, [authorized], {
      confirmations: 0,
      skipPreflight: true,
    }),
  ).rejects.toThrow();

  // Split stake
  const newStake = new Account();
  let split = StakeProgram.split({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    splitStakePubkey: newStake.publicKey,
    lamports: minimumAmount + 20,
  });
  await sendAndConfirmTransaction(connection, split, [authorized, newStake], {
    confirmations: 0,
    skipPreflight: true,
  });

  // Authorize to new account
  const newAuthorized = new Account();
  await connection.requestAirdrop(newAuthorized.publicKey, LAMPORTS_PER_SOL);

  let authorize = StakeProgram.authorize({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    newAuthorizedPubkey: newAuthorized.publicKey,
    stakeAuthorizationType: StakeAuthorizationLayout.Withdrawer,
  });
  await sendAndConfirmTransaction(connection, authorize, [authorized], {
    confirmations: 0,
    skipPreflight: true,
  });
  authorize = StakeProgram.authorize({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
    newAuthorizedPubkey: newAuthorized.publicKey,
    stakeAuthorizationType: StakeAuthorizationLayout.Staker,
  });
  await sendAndConfirmTransaction(connection, authorize, [authorized], {
    confirmations: 0,
    skipPreflight: true,
  });

  // Test old authorized can't deactivate
  let deactivateNotAuthorized = StakeProgram.deactivate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: authorized.publicKey,
  });
  await expect(
    sendAndConfirmTransaction(
      connection,
      deactivateNotAuthorized,
      [authorized],
      {confirmations: 0, skipPreflight: true},
    ),
  ).rejects.toThrow();

  // Deactivate stake
  let deactivate = StakeProgram.deactivate({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: newAuthorized.publicKey,
  });
  await sendAndConfirmTransaction(connection, deactivate, [newAuthorized], {
    confirmations: 0,
    skipPreflight: true,
  });

  // Test that withdraw succeeds after deactivation
  withdraw = StakeProgram.withdraw({
    stakePubkey: newAccountPubkey,
    authorizedPubkey: newAuthorized.publicKey,
    toPubkey: recipient.publicKey,
    lamports: minimumAmount + 20,
  });
  await sendAndConfirmTransaction(connection, withdraw, [newAuthorized], {
    confirmations: 0,
    skipPreflight: true,
  });
  const balance = await connection.getBalance(newAccountPubkey);
  expect(balance).toEqual(minimumAmount + 2);
  const recipientBalance = await connection.getBalance(recipient.publicKey);
  expect(recipientBalance).toEqual(minimumAmount + 20);
});
