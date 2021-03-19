import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

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
import {helpers} from './mocks/rpc-http';
import {url} from './url';

use(chaiAsPromised);

describe('StakeProgram', () => {
  it('createAccountWithSeed', async () => {
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
    expect(transaction.instructions).to.have.length(2);
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
    expect(systemParams).to.eql(
      SystemInstruction.decodeCreateWithSeed(systemInstruction),
    );
    const initParams = {stakePubkey: newAccountPubkey, authorized, lockup};
    expect(initParams).to.eql(
      StakeInstruction.decodeInitialize(stakeInstruction),
    );
  });

  it('createAccount', () => {
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
    expect(transaction.instructions).to.have.length(2);
    const [systemInstruction, stakeInstruction] = transaction.instructions;
    const systemParams = {
      fromPubkey,
      newAccountPubkey,
      lamports,
      space: StakeProgram.space,
      programId: StakeProgram.programId,
    };
    expect(systemParams).to.eql(
      SystemInstruction.decodeCreateAccount(systemInstruction),
    );

    const initParams = {stakePubkey: newAccountPubkey, authorized, lockup};
    expect(initParams).to.eql(
      StakeInstruction.decodeInitialize(stakeInstruction),
    );
  });

  it('delegate', () => {
    const stakePubkey = new Account().publicKey;
    const authorizedPubkey = new Account().publicKey;
    const votePubkey = new Account().publicKey;
    const params = {
      stakePubkey,
      authorizedPubkey,
      votePubkey,
    };
    const transaction = StakeProgram.delegate(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeDelegate(stakeInstruction));
  });

  it('authorize', () => {
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
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeAuthorize(stakeInstruction));
  });

  it('authorize with custodian', () => {
    const stakePubkey = new Account().publicKey;
    const authorizedPubkey = new Account().publicKey;
    const newAuthorizedPubkey = new Account().publicKey;
    const stakeAuthorizationType = StakeAuthorizationLayout.Withdrawer;
    const custodianPubkey = new Account().publicKey;
    const params = {
      stakePubkey,
      authorizedPubkey,
      newAuthorizedPubkey,
      stakeAuthorizationType,
      custodianPubkey,
    };
    const transaction = StakeProgram.authorize(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeAuthorize(stakeInstruction));
  });

  it('authorizeWithSeed', () => {
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
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(
      StakeInstruction.decodeAuthorizeWithSeed(stakeInstruction),
    );
  });

  it('authorizeWithSeed with custodian', () => {
    const stakePubkey = new Account().publicKey;
    const authorityBase = new Account().publicKey;
    const authoritySeed = 'test string';
    const authorityOwner = new Account().publicKey;
    const newAuthorizedPubkey = new Account().publicKey;
    const stakeAuthorizationType = StakeAuthorizationLayout.Staker;
    const custodianPubkey = new Account().publicKey;
    const params = {
      stakePubkey,
      authorityBase,
      authoritySeed,
      authorityOwner,
      newAuthorizedPubkey,
      stakeAuthorizationType,
      custodianPubkey,
    };
    const transaction = StakeProgram.authorizeWithSeed(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(
      StakeInstruction.decodeAuthorizeWithSeed(stakeInstruction),
    );
  });

  it('split', () => {
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
    expect(transaction.instructions).to.have.length(2);
    const [systemInstruction, stakeInstruction] = transaction.instructions;
    const systemParams = {
      fromPubkey: authorizedPubkey,
      newAccountPubkey: splitStakePubkey,
      lamports: 0,
      space: StakeProgram.space,
      programId: StakeProgram.programId,
    };
    expect(systemParams).to.eql(
      SystemInstruction.decodeCreateAccount(systemInstruction),
    );
    expect(params).to.eql(StakeInstruction.decodeSplit(stakeInstruction));
  });

  it('withdraw', () => {
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
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeWithdraw(stakeInstruction));
  });

  it('withdraw with custodian', () => {
    const stakePubkey = new Account().publicKey;
    const authorizedPubkey = new Account().publicKey;
    const toPubkey = new Account().publicKey;
    const custodianPubkey = new Account().publicKey;
    const params = {
      stakePubkey,
      authorizedPubkey,
      toPubkey,
      lamports: 123,
      custodianPubkey,
    };
    const transaction = StakeProgram.withdraw(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeWithdraw(stakeInstruction));
  });

  it('deactivate', () => {
    const stakePubkey = new Account().publicKey;
    const authorizedPubkey = new Account().publicKey;
    const params = {stakePubkey, authorizedPubkey};
    const transaction = StakeProgram.deactivate(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeDeactivate(stakeInstruction));
  });

  it('StakeInstructions', async () => {
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

    expect(createWithSeedTransaction.instructions).to.have.length(2);
    const systemInstructionType = SystemInstruction.decodeInstructionType(
      createWithSeedTransaction.instructions[0],
    );
    expect(systemInstructionType).to.eq('CreateWithSeed');

    const stakeInstructionType = StakeInstruction.decodeInstructionType(
      createWithSeedTransaction.instructions[1],
    );
    expect(stakeInstructionType).to.eq('Initialize');

    expect(() => {
      StakeInstruction.decodeInstructionType(
        createWithSeedTransaction.instructions[0],
      );
    }).to.throw();

    const stake = new Account();
    const vote = new Account();
    const delegate = StakeProgram.delegate({
      stakePubkey: stake.publicKey,
      authorizedPubkey: authorized.publicKey,
      votePubkey: vote.publicKey,
    });

    const delegateTransaction = new Transaction({recentBlockhash}).add(
      delegate,
    );
    const anotherStakeInstructionType = StakeInstruction.decodeInstructionType(
      delegateTransaction.instructions[0],
    );
    expect(anotherStakeInstructionType).to.eq('Delegate');
  });

  if (process.env.TEST_LIVE) {
    it('live staking actions', async () => {
      const connection = new Connection(url, 'confirmed');

      const voteAccounts = await connection.getVoteAccounts();
      const voteAccount = voteAccounts.current.concat(
        voteAccounts.delinquent,
      )[0];
      const votePubkey = new PublicKey(voteAccount.votePubkey);

      const payer = new Account();
      await helpers.airdrop({
        connection,
        address: payer.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });

      const authorized = new Account();
      await helpers.airdrop({
        connection,
        address: authorized.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });

      const minimumAmount = await connection.getMinimumBalanceForRentExemption(
        StakeProgram.space,
      );

      expect(await connection.getBalance(payer.publicKey)).to.eq(
        2 * LAMPORTS_PER_SOL,
      );
      expect(await connection.getBalance(authorized.publicKey)).to.eq(
        2 * LAMPORTS_PER_SOL,
      );

      {
        // Create Stake account without seed
        const newStakeAccount = new Account();
        let createAndInitialize = StakeProgram.createAccount({
          fromPubkey: payer.publicKey,
          stakePubkey: newStakeAccount.publicKey,
          authorized: new Authorized(
            authorized.publicKey,
            authorized.publicKey,
          ),
          lockup: new Lockup(0, 0, new PublicKey(0)),
          lamports: minimumAmount + 42,
        });

        await sendAndConfirmTransaction(
          connection,
          createAndInitialize,
          [payer, newStakeAccount],
          {preflightCommitment: 'confirmed'},
        );
        expect(await connection.getBalance(newStakeAccount.publicKey)).to.eq(
          minimumAmount + 42,
        );

        const delegation = StakeProgram.delegate({
          stakePubkey: newStakeAccount.publicKey,
          authorizedPubkey: authorized.publicKey,
          votePubkey,
        });
        await sendAndConfirmTransaction(connection, delegation, [authorized], {
          commitment: 'confirmed',
        });
      }

      // Create Stake account with seed
      const seed = 'test string';
      const newAccountPubkey = await PublicKey.createWithSeed(
        payer.publicKey,
        seed,
        StakeProgram.programId,
      );

      let createAndInitializeWithSeed = StakeProgram.createAccountWithSeed({
        fromPubkey: payer.publicKey,
        stakePubkey: newAccountPubkey,
        basePubkey: payer.publicKey,
        seed,
        authorized: new Authorized(authorized.publicKey, authorized.publicKey),
        lockup: new Lockup(0, 0, new PublicKey(0)),
        lamports: 3 * minimumAmount + 42,
      });

      await sendAndConfirmTransaction(
        connection,
        createAndInitializeWithSeed,
        [payer],
        {preflightCommitment: 'confirmed'},
      );
      let originalStakeBalance = await connection.getBalance(newAccountPubkey);
      expect(originalStakeBalance).to.eq(3 * minimumAmount + 42);

      let delegation = StakeProgram.delegate({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        votePubkey,
      });
      await sendAndConfirmTransaction(connection, delegation, [authorized], {
        preflightCommitment: 'confirmed',
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
          preflightCommitment: 'confirmed',
        }),
      ).to.be.rejected;

      // Deactivate stake
      let deactivate = StakeProgram.deactivate({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
      });
      await sendAndConfirmTransaction(connection, deactivate, [authorized], {
        preflightCommitment: 'confirmed',
      });

      let stakeActivationState;
      do {
        stakeActivationState = await connection.getStakeActivation(
          newAccountPubkey,
        );
      } while (stakeActivationState.state != 'inactive');

      // Test that withdraw succeeds after deactivation
      withdraw = StakeProgram.withdraw({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        toPubkey: recipient.publicKey,
        lamports: minimumAmount + 20,
      });

      await sendAndConfirmTransaction(connection, withdraw, [authorized], {
        preflightCommitment: 'confirmed',
      });
      const recipientBalance = await connection.getBalance(recipient.publicKey);
      expect(recipientBalance).to.eq(minimumAmount + 20);

      // Split stake
      const newStake = new Account();
      let split = StakeProgram.split({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        splitStakePubkey: newStake.publicKey,
        lamports: minimumAmount + 20,
      });
      await sendAndConfirmTransaction(
        connection,
        split,
        [authorized, newStake],
        {
          preflightCommitment: 'confirmed',
        },
      );
      const balance = await connection.getBalance(newAccountPubkey);
      expect(balance).to.eq(minimumAmount + 2);

      // Authorize to new account
      const newAuthorized = new Account();
      await connection.requestAirdrop(
        newAuthorized.publicKey,
        LAMPORTS_PER_SOL,
      );

      let authorize = StakeProgram.authorize({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        newAuthorizedPubkey: newAuthorized.publicKey,
        stakeAuthorizationType: StakeAuthorizationLayout.Withdrawer,
      });
      await sendAndConfirmTransaction(connection, authorize, [authorized], {
        preflightCommitment: 'confirmed',
      });
      authorize = StakeProgram.authorize({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        newAuthorizedPubkey: newAuthorized.publicKey,
        stakeAuthorizationType: StakeAuthorizationLayout.Staker,
      });
      await sendAndConfirmTransaction(connection, authorize, [authorized], {
        preflightCommitment: 'confirmed',
      });

      // Test old authorized can't delegate
      let delegateNotAuthorized = StakeProgram.delegate({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: authorized.publicKey,
        votePubkey,
      });
      await expect(
        sendAndConfirmTransaction(
          connection,
          delegateNotAuthorized,
          [authorized],
          {
            preflightCommitment: 'confirmed',
          },
        ),
      ).to.be.rejected;

      // Authorize a derived address
      authorize = StakeProgram.authorize({
        stakePubkey: newAccountPubkey,
        authorizedPubkey: newAuthorized.publicKey,
        newAuthorizedPubkey: newAccountPubkey,
        stakeAuthorizationType: StakeAuthorizationLayout.Withdrawer,
      });
      await sendAndConfirmTransaction(connection, authorize, [newAuthorized], {
        preflightCommitment: 'confirmed',
      });

      // Restore the previous authority using a derived address
      authorize = StakeProgram.authorizeWithSeed({
        stakePubkey: newAccountPubkey,
        authorityBase: payer.publicKey,
        authoritySeed: seed,
        authorityOwner: StakeProgram.programId,
        newAuthorizedPubkey: newAuthorized.publicKey,
        stakeAuthorizationType: StakeAuthorizationLayout.Withdrawer,
      });
      await sendAndConfirmTransaction(connection, authorize, [payer], {
        preflightCommitment: 'confirmed',
      });
    }).timeout(10 * 1000);
  }
});
