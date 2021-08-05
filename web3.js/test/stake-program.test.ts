import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Keypair,
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
    const fromPubkey = Keypair.generate().publicKey;
    const seed = 'test string';
    const newAccountPubkey = await PublicKey.createWithSeed(
      fromPubkey,
      seed,
      StakeProgram.programId,
    );
    const authorizedPubkey = Keypair.generate().publicKey;
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
    const fromPubkey = Keypair.generate().publicKey;
    const newAccountPubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const votePubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const newAuthorizedPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const newAuthorizedPubkey = Keypair.generate().publicKey;
    const stakeAuthorizationType = StakeAuthorizationLayout.Withdrawer;
    const custodianPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorityBase = Keypair.generate().publicKey;
    const authoritySeed = 'test string';
    const authorityOwner = Keypair.generate().publicKey;
    const newAuthorizedPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorityBase = Keypair.generate().publicKey;
    const authoritySeed = 'test string';
    const authorityOwner = Keypair.generate().publicKey;
    const newAuthorizedPubkey = Keypair.generate().publicKey;
    const stakeAuthorizationType = StakeAuthorizationLayout.Staker;
    const custodianPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const splitStakePubkey = Keypair.generate().publicKey;
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

  it('merge', () => {
    const stakePubkey = Keypair.generate().publicKey;
    const sourceStakePubKey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const params = {
      stakePubkey,
      sourceStakePubKey,
      authorizedPubkey,
    };
    const transaction = StakeProgram.merge(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeMerge(stakeInstruction));
  });

  it('withdraw', () => {
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const toPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const toPubkey = Keypair.generate().publicKey;
    const custodianPubkey = Keypair.generate().publicKey;
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
    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const params = {stakePubkey, authorizedPubkey};
    const transaction = StakeProgram.deactivate(params);
    expect(transaction.instructions).to.have.length(1);
    const [stakeInstruction] = transaction.instructions;
    expect(params).to.eql(StakeInstruction.decodeDeactivate(stakeInstruction));
  });

  it('StakeInstructions', async () => {
    const from = Keypair.generate();
    const seed = 'test string';
    const newAccountPubkey = await PublicKey.createWithSeed(
      from.publicKey,
      seed,
      StakeProgram.programId,
    );
    const authorized = Keypair.generate();
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

    const stake = Keypair.generate();
    const vote = Keypair.generate();
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

      const payer = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: payer.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });

      const authorized = Keypair.generate();
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
        const newStakeAccount = Keypair.generate();
        let createAndInitialize = StakeProgram.createAccount({
          fromPubkey: payer.publicKey,
          stakePubkey: newStakeAccount.publicKey,
          authorized: new Authorized(
            authorized.publicKey,
            authorized.publicKey,
          ),
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
      const recipient = Keypair.generate();
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
      const newStake = Keypair.generate();
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
      const newAuthorized = Keypair.generate();
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
