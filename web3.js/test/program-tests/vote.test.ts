import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Keypair,
  LAMPORTS_PER_SOL,
  VoteAuthorizationLayout,
  VoteInit,
  VoteInstruction,
  VoteProgram,
  sendAndConfirmTransaction,
  SystemInstruction,
  Connection,
  PublicKey,
} from '../../src';
import {helpers} from '../mocks/rpc-http';
import {url} from '../url';

use(chaiAsPromised);

describe('VoteProgram', () => {
  it('createAccount', () => {
    const fromPubkey = Keypair.generate().publicKey;
    const newAccountPubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const nodePubkey = Keypair.generate().publicKey;
    const commission = 5;
    const voteInit = new VoteInit(
      nodePubkey,
      authorizedPubkey,
      authorizedPubkey,
      commission,
    );
    const lamports = 123;
    const transaction = VoteProgram.createAccount({
      fromPubkey,
      votePubkey: newAccountPubkey,
      voteInit,
      lamports,
    });
    expect(transaction.instructions).to.have.length(2);
    const [systemInstruction, voteInstruction] = transaction.instructions;
    const systemParams = {
      fromPubkey,
      newAccountPubkey,
      lamports,
      space: VoteProgram.space,
      programId: VoteProgram.programId,
    };
    expect(systemParams).to.eql(
      SystemInstruction.decodeCreateAccount(systemInstruction),
    );

    const initParams = {votePubkey: newAccountPubkey, nodePubkey, voteInit};
    expect(initParams).to.eql(
      VoteInstruction.decodeInitializeAccount(voteInstruction),
    );
  });

  it('initialize', () => {
    const newAccountPubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const nodePubkey = Keypair.generate().publicKey;
    const voteInit = new VoteInit(
      nodePubkey,
      authorizedPubkey,
      authorizedPubkey,
      5,
    );
    const initParams = {
      votePubkey: newAccountPubkey,
      nodePubkey,
      voteInit,
    };
    const initInstruction = VoteProgram.initializeAccount(initParams);
    expect(initParams).to.eql(
      VoteInstruction.decodeInitializeAccount(initInstruction),
    );
  });

  it('authorize', () => {
    const votePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const newAuthorizedPubkey = Keypair.generate().publicKey;
    const voteAuthorizationType = VoteAuthorizationLayout.Voter;
    const params = {
      votePubkey,
      authorizedPubkey,
      newAuthorizedPubkey,
      voteAuthorizationType,
    };
    const transaction = VoteProgram.authorize(params);
    expect(transaction.instructions).to.have.length(1);
    const [authorizeInstruction] = transaction.instructions;
    expect(params).to.eql(
      VoteInstruction.decodeAuthorize(authorizeInstruction),
    );
  });

  it('authorize with seed', () => {
    const votePubkey = Keypair.generate().publicKey;
    const currentAuthorityDerivedKeyBasePubkey = Keypair.generate().publicKey;
    const currentAuthorityDerivedKeyOwnerPubkey = Keypair.generate().publicKey;
    const currentAuthorityDerivedKeySeed = 'sunflower';
    const newAuthorizedPubkey = Keypair.generate().publicKey;
    const voteAuthorizationType = VoteAuthorizationLayout.Voter;
    const params = {
      currentAuthorityDerivedKeyBasePubkey,
      currentAuthorityDerivedKeyOwnerPubkey,
      currentAuthorityDerivedKeySeed,
      newAuthorizedPubkey,
      voteAuthorizationType,
      votePubkey,
    };
    const transaction = VoteProgram.authorizeWithSeed(params);
    expect(transaction.instructions).to.have.length(1);
    const [authorizeWithSeedInstruction] = transaction.instructions;
    expect(params).to.eql(
      VoteInstruction.decodeAuthorizeWithSeed(authorizeWithSeedInstruction),
    );
  });

  it('withdraw', () => {
    const votePubkey = Keypair.generate().publicKey;
    const authorizedWithdrawerPubkey = Keypair.generate().publicKey;
    const toPubkey = Keypair.generate().publicKey;
    const params = {
      votePubkey,
      authorizedWithdrawerPubkey,
      lamports: 123,
      toPubkey,
    };
    const transaction = VoteProgram.withdraw(params);
    expect(transaction.instructions).to.have.length(1);
    const [withdrawInstruction] = transaction.instructions;
    expect(params).to.eql(VoteInstruction.decodeWithdraw(withdrawInstruction));
  });

  if (process.env.TEST_LIVE) {
    it('change authority from derived key', async () => {
      const connection = new Connection(url, 'confirmed');

      const newVoteAccount = Keypair.generate();
      const nodeAccount = Keypair.generate();
      const derivedKeyOwnerProgram = Keypair.generate();
      const derivedKeySeed = 'sunflower';
      const newAuthorizedWithdrawer = Keypair.generate();

      const derivedKeyBaseKeypair = Keypair.generate();
      const [
        _1, // eslint-disable-line @typescript-eslint/no-unused-vars
        _2, // eslint-disable-line @typescript-eslint/no-unused-vars
        minimumAmount,
        derivedKey,
      ] = await Promise.all([
        (async () => {
          await helpers.airdrop({
            connection,
            address: derivedKeyBaseKeypair.publicKey,
            amount: 12 * LAMPORTS_PER_SOL,
          });
          expect(
            await connection.getBalance(derivedKeyBaseKeypair.publicKey),
          ).to.eq(12 * LAMPORTS_PER_SOL);
        })(),
        (async () => {
          await helpers.airdrop({
            connection,
            address: newAuthorizedWithdrawer.publicKey,
            amount: 0.1 * LAMPORTS_PER_SOL,
          });
          expect(
            await connection.getBalance(newAuthorizedWithdrawer.publicKey),
          ).to.eq(0.1 * LAMPORTS_PER_SOL);
        })(),
        connection.getMinimumBalanceForRentExemption(VoteProgram.space),
        PublicKey.createWithSeed(
          derivedKeyBaseKeypair.publicKey,
          derivedKeySeed,
          derivedKeyOwnerProgram.publicKey,
        ),
      ]);

      // Create initialized Vote account
      const createAndInitialize = VoteProgram.createAccount({
        fromPubkey: derivedKeyBaseKeypair.publicKey,
        votePubkey: newVoteAccount.publicKey,
        voteInit: new VoteInit(
          nodeAccount.publicKey,
          derivedKey,
          derivedKey,
          5,
        ),
        lamports: minimumAmount + 10 * LAMPORTS_PER_SOL,
      });
      await sendAndConfirmTransaction(
        connection,
        createAndInitialize,
        [derivedKeyBaseKeypair, newVoteAccount, nodeAccount],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(newVoteAccount.publicKey)).to.eq(
        minimumAmount + 10 * LAMPORTS_PER_SOL,
      );

      // Authorize a new Withdrawer.
      const authorize = VoteProgram.authorizeWithSeed({
        currentAuthorityDerivedKeyBasePubkey: derivedKeyBaseKeypair.publicKey,
        currentAuthorityDerivedKeyOwnerPubkey: derivedKeyOwnerProgram.publicKey,
        currentAuthorityDerivedKeySeed: derivedKeySeed,
        newAuthorizedPubkey: newAuthorizedWithdrawer.publicKey,
        voteAuthorizationType: VoteAuthorizationLayout.Withdrawer,
        votePubkey: newVoteAccount.publicKey,
      });
      await sendAndConfirmTransaction(
        connection,
        authorize,
        [derivedKeyBaseKeypair],
        {preflightCommitment: 'confirmed'},
      );

      // Test newAuthorizedWithdrawer may withdraw.
      const recipient = Keypair.generate();
      const withdraw = VoteProgram.withdraw({
        votePubkey: newVoteAccount.publicKey,
        authorizedWithdrawerPubkey: newAuthorizedWithdrawer.publicKey,
        lamports: LAMPORTS_PER_SOL,
        toPubkey: recipient.publicKey,
      });
      await sendAndConfirmTransaction(
        connection,
        withdraw,
        [newAuthorizedWithdrawer],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(recipient.publicKey)).to.eq(
        LAMPORTS_PER_SOL,
      );
    });

    it('live vote actions', async () => {
      const connection = new Connection(url, 'confirmed');

      const newVoteAccount = Keypair.generate();
      const nodeAccount = Keypair.generate();

      const payer = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: payer.publicKey,
        amount: 12 * LAMPORTS_PER_SOL,
      });
      expect(await connection.getBalance(payer.publicKey)).to.eq(
        12 * LAMPORTS_PER_SOL,
      );

      const authorized = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: authorized.publicKey,
        amount: 12 * LAMPORTS_PER_SOL,
      });
      expect(await connection.getBalance(authorized.publicKey)).to.eq(
        12 * LAMPORTS_PER_SOL,
      );

      const minimumAmount = await connection.getMinimumBalanceForRentExemption(
        VoteProgram.space,
      );

      // Create initialized Vote account
      let createAndInitialize = VoteProgram.createAccount({
        fromPubkey: payer.publicKey,
        votePubkey: newVoteAccount.publicKey,
        voteInit: new VoteInit(
          nodeAccount.publicKey,
          authorized.publicKey,
          authorized.publicKey,
          5,
        ),
        lamports: minimumAmount + 10 * LAMPORTS_PER_SOL,
      });
      await sendAndConfirmTransaction(
        connection,
        createAndInitialize,
        [payer, newVoteAccount, nodeAccount],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(newVoteAccount.publicKey)).to.eq(
        minimumAmount + 10 * LAMPORTS_PER_SOL,
      );

      // Withdraw from Vote account
      let recipient = Keypair.generate();
      const voteBalance = await connection.getBalance(newVoteAccount.publicKey);

      expect(() =>
        VoteProgram.safeWithdraw(
          {
            votePubkey: newVoteAccount.publicKey,
            authorizedWithdrawerPubkey: authorized.publicKey,
            lamports: voteBalance - minimumAmount + 1,
            toPubkey: recipient.publicKey,
          },
          voteBalance,
          minimumAmount,
        ),
      ).to.throw('Withdraw will leave vote account with insuffcient funds.');

      let withdraw = VoteProgram.withdraw({
        votePubkey: newVoteAccount.publicKey,
        authorizedWithdrawerPubkey: authorized.publicKey,
        lamports: LAMPORTS_PER_SOL,
        toPubkey: recipient.publicKey,
      });
      await sendAndConfirmTransaction(connection, withdraw, [authorized], {
        preflightCommitment: 'confirmed',
      });
      expect(await connection.getBalance(recipient.publicKey)).to.eq(
        LAMPORTS_PER_SOL,
      );

      const newAuthorizedWithdrawer = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: newAuthorizedWithdrawer.publicKey,
        amount: LAMPORTS_PER_SOL,
      });
      expect(
        await connection.getBalance(newAuthorizedWithdrawer.publicKey),
      ).to.eq(LAMPORTS_PER_SOL);

      // Authorize a new Withdrawer.
      let authorize = VoteProgram.authorize({
        votePubkey: newVoteAccount.publicKey,
        authorizedPubkey: authorized.publicKey,
        newAuthorizedPubkey: newAuthorizedWithdrawer.publicKey,
        voteAuthorizationType: VoteAuthorizationLayout.Withdrawer,
      });
      await sendAndConfirmTransaction(connection, authorize, [authorized], {
        preflightCommitment: 'confirmed',
      });

      // Test old authorized cannot withdraw anymore.
      withdraw = VoteProgram.withdraw({
        votePubkey: newVoteAccount.publicKey,
        authorizedWithdrawerPubkey: authorized.publicKey,
        lamports: minimumAmount,
        toPubkey: recipient.publicKey,
      });
      await expect(
        sendAndConfirmTransaction(connection, withdraw, [authorized], {
          preflightCommitment: 'confirmed',
        }),
      ).to.be.rejected;

      // Test newAuthorizedWithdrawer may withdraw.
      recipient = Keypair.generate();
      withdraw = VoteProgram.withdraw({
        votePubkey: newVoteAccount.publicKey,
        authorizedWithdrawerPubkey: newAuthorizedWithdrawer.publicKey,
        lamports: LAMPORTS_PER_SOL,
        toPubkey: recipient.publicKey,
      });
      await sendAndConfirmTransaction(
        connection,
        withdraw,
        [newAuthorizedWithdrawer],
        {
          preflightCommitment: 'confirmed',
        },
      );
      expect(await connection.getBalance(recipient.publicKey)).to.eq(
        LAMPORTS_PER_SOL,
      );

      const newAuthorizedVoter = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: newAuthorizedVoter.publicKey,
        amount: LAMPORTS_PER_SOL,
      });
      expect(await connection.getBalance(newAuthorizedVoter.publicKey)).to.eq(
        LAMPORTS_PER_SOL,
      );

      // The authorized Withdrawer may sign to authorize a new Voter, see
      // https://github.com/solana-labs/solana/issues/22521
      authorize = VoteProgram.authorize({
        votePubkey: newVoteAccount.publicKey,
        authorizedPubkey: newAuthorizedWithdrawer.publicKey,
        newAuthorizedPubkey: newAuthorizedVoter.publicKey,
        voteAuthorizationType: VoteAuthorizationLayout.Voter,
      });
      await sendAndConfirmTransaction(
        connection,
        authorize,
        [newAuthorizedWithdrawer],
        {
          preflightCommitment: 'confirmed',
        },
      );
    }).timeout(10 * 1000);
  }
});
