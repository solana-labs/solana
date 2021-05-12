import {Buffer} from 'buffer';
import {expect} from 'chai';

import {
  Keypair,
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
import {sleep} from '../src/util/sleep';
import {helpers} from './mocks/rpc-http';
import {url} from './url';

describe('SystemProgram', () => {
  it('createAccount', () => {
    const params = {
      fromPubkey: Keypair.generate().publicKey,
      newAccountPubkey: Keypair.generate().publicKey,
      lamports: 123,
      space: 0,
      programId: SystemProgram.programId,
    };
    const transaction = new Transaction().add(
      SystemProgram.createAccount(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(
      SystemInstruction.decodeCreateAccount(systemInstruction),
    );
  });

  it('transfer', () => {
    const params = {
      fromPubkey: Keypair.generate().publicKey,
      toPubkey: Keypair.generate().publicKey,
      lamports: 123,
    };
    const transaction = new Transaction().add(SystemProgram.transfer(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(SystemInstruction.decodeTransfer(systemInstruction));
  });

  it('transferWithSeed', () => {
    const params = {
      fromPubkey: Keypair.generate().publicKey,
      basePubkey: Keypair.generate().publicKey,
      toPubkey: Keypair.generate().publicKey,
      lamports: 123,
      seed: '你好',
      programId: Keypair.generate().publicKey,
    };
    const transaction = new Transaction().add(SystemProgram.transfer(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(
      SystemInstruction.decodeTransferWithSeed(systemInstruction),
    );
  });

  it('allocate', () => {
    const params = {
      accountPubkey: Keypair.generate().publicKey,
      space: 42,
    };
    const transaction = new Transaction().add(SystemProgram.allocate(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(SystemInstruction.decodeAllocate(systemInstruction));
  });

  it('allocateWithSeed', () => {
    const params = {
      accountPubkey: Keypair.generate().publicKey,
      basePubkey: Keypair.generate().publicKey,
      seed: '你好',
      space: 42,
      programId: Keypair.generate().publicKey,
    };
    const transaction = new Transaction().add(SystemProgram.allocate(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(
      SystemInstruction.decodeAllocateWithSeed(systemInstruction),
    );
  });

  it('assign', () => {
    const params = {
      accountPubkey: Keypair.generate().publicKey,
      programId: Keypair.generate().publicKey,
    };
    const transaction = new Transaction().add(SystemProgram.assign(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(SystemInstruction.decodeAssign(systemInstruction));
  });

  it('assignWithSeed', () => {
    const params = {
      accountPubkey: Keypair.generate().publicKey,
      basePubkey: Keypair.generate().publicKey,
      seed: '你好',
      programId: Keypair.generate().publicKey,
    };
    const transaction = new Transaction().add(SystemProgram.assign(params));
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(
      SystemInstruction.decodeAssignWithSeed(systemInstruction),
    );
  });

  it('createAccountWithSeed', () => {
    const fromPubkey = Keypair.generate().publicKey;
    const params = {
      fromPubkey,
      newAccountPubkey: Keypair.generate().publicKey,
      basePubkey: fromPubkey,
      seed: 'hi there',
      lamports: 123,
      space: 0,
      programId: SystemProgram.programId,
    };
    const transaction = new Transaction().add(
      SystemProgram.createAccountWithSeed(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [systemInstruction] = transaction.instructions;
    expect(params).to.eql(
      SystemInstruction.decodeCreateWithSeed(systemInstruction),
    );
  });

  it('createNonceAccount', () => {
    const fromPubkey = Keypair.generate().publicKey;
    const params = {
      fromPubkey,
      noncePubkey: Keypair.generate().publicKey,
      authorizedPubkey: fromPubkey,
      lamports: 123,
    };

    const transaction = new Transaction().add(
      SystemProgram.createNonceAccount(params),
    );
    expect(transaction.instructions).to.have.length(2);
    const [createInstruction, initInstruction] = transaction.instructions;

    const createParams = {
      fromPubkey: params.fromPubkey,
      newAccountPubkey: params.noncePubkey,
      lamports: params.lamports,
      space: NONCE_ACCOUNT_LENGTH,
      programId: SystemProgram.programId,
    };
    expect(createParams).to.eql(
      SystemInstruction.decodeCreateAccount(createInstruction),
    );

    const initParams = {
      noncePubkey: params.noncePubkey,
      authorizedPubkey: fromPubkey,
    };
    expect(initParams).to.eql(
      SystemInstruction.decodeNonceInitialize(initInstruction),
    );
  });

  it('createNonceAccount with seed', () => {
    const fromPubkey = Keypair.generate().publicKey;
    const params = {
      fromPubkey,
      noncePubkey: Keypair.generate().publicKey,
      authorizedPubkey: fromPubkey,
      basePubkey: fromPubkey,
      seed: 'hi there',
      lamports: 123,
    };

    const transaction = new Transaction().add(
      SystemProgram.createNonceAccount(params),
    );
    expect(transaction.instructions).to.have.length(2);
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
    expect(createParams).to.eql(
      SystemInstruction.decodeCreateWithSeed(createInstruction),
    );

    const initParams = {
      noncePubkey: params.noncePubkey,
      authorizedPubkey: fromPubkey,
    };
    expect(initParams).to.eql(
      SystemInstruction.decodeNonceInitialize(initInstruction),
    );
  });

  it('nonceAdvance', () => {
    const params = {
      noncePubkey: Keypair.generate().publicKey,
      authorizedPubkey: Keypair.generate().publicKey,
    };
    const instruction = SystemProgram.nonceAdvance(params);
    expect(params).to.eql(SystemInstruction.decodeNonceAdvance(instruction));
  });

  it('nonceWithdraw', () => {
    const params = {
      noncePubkey: Keypair.generate().publicKey,
      authorizedPubkey: Keypair.generate().publicKey,
      toPubkey: Keypair.generate().publicKey,
      lamports: 123,
    };
    const transaction = new Transaction().add(
      SystemProgram.nonceWithdraw(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [instruction] = transaction.instructions;
    expect(params).to.eql(SystemInstruction.decodeNonceWithdraw(instruction));
  });

  it('nonceAuthorize', () => {
    const params = {
      noncePubkey: Keypair.generate().publicKey,
      authorizedPubkey: Keypair.generate().publicKey,
      newAuthorizedPubkey: Keypair.generate().publicKey,
    };

    const transaction = new Transaction().add(
      SystemProgram.nonceAuthorize(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [instruction] = transaction.instructions;
    expect(params).to.eql(SystemInstruction.decodeNonceAuthorize(instruction));
  });

  it('non-SystemInstruction error', () => {
    const from = Keypair.generate();
    const to = Keypair.generate();

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
    }).to.throw();

    const stakePubkey = Keypair.generate().publicKey;
    const authorizedPubkey = Keypair.generate().publicKey;
    const params = {stakePubkey, authorizedPubkey};
    const transaction = StakeProgram.deactivate(params);

    expect(() => {
      SystemInstruction.decodeInstructionType(transaction.instructions[1]);
    }).to.throw();

    transaction.instructions[0].data[0] = 11;
    expect(() => {
      SystemInstruction.decodeInstructionType(transaction.instructions[0]);
    }).to.throw();
  });

  if (process.env.TEST_LIVE) {
    it('live Nonce actions', async () => {
      const connection = new Connection(url, 'confirmed');
      const nonceAccount = Keypair.generate();
      const from = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: from.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });

      const to = Keypair.generate();
      const newAuthority = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: newAuthority.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

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
        {preflightCommitment: 'confirmed'},
      );
      const nonceBalance = await connection.getBalance(nonceAccount.publicKey);
      expect(nonceBalance).to.eq(minimumAmount);

      const nonceQuery1 = await connection.getNonce(nonceAccount.publicKey);
      if (nonceQuery1 === null) {
        expect(nonceQuery1).not.to.be.null;
        return;
      }

      const nonceQuery2 = await connection.getNonce(nonceAccount.publicKey);
      if (nonceQuery2 === null) {
        expect(nonceQuery2).not.to.be.null;
        return;
      }

      expect(nonceQuery1.nonce).to.eq(nonceQuery2.nonce);

      // Wait for blockhash to advance
      await sleep(500);

      const advanceNonce = new Transaction().add(
        SystemProgram.nonceAdvance({
          noncePubkey: nonceAccount.publicKey,
          authorizedPubkey: from.publicKey,
        }),
      );
      await sendAndConfirmTransaction(connection, advanceNonce, [from], {
        preflightCommitment: 'confirmed',
      });
      const nonceQuery3 = await connection.getNonce(nonceAccount.publicKey);
      if (nonceQuery3 === null) {
        expect(nonceQuery3).not.to.be.null;
        return;
      }
      expect(nonceQuery1.nonce).not.to.eq(nonceQuery3.nonce);
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
        preflightCommitment: 'confirmed',
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

      await sendAndConfirmTransaction(
        connection,
        transfer,
        [from, newAuthority],
        {
          preflightCommitment: 'confirmed',
        },
      );
      const toBalance = await connection.getBalance(to.publicKey);
      expect(toBalance).to.eq(minimumAmount);

      // Wait for blockhash to advance
      await sleep(500);

      const withdrawAccount = Keypair.generate();
      const withdrawNonce = new Transaction().add(
        SystemProgram.nonceWithdraw({
          noncePubkey: nonceAccount.publicKey,
          authorizedPubkey: newAuthority.publicKey,
          lamports: minimumAmount,
          toPubkey: withdrawAccount.publicKey,
        }),
      );
      await sendAndConfirmTransaction(
        connection,
        withdrawNonce,
        [newAuthority],
        {
          preflightCommitment: 'confirmed',
        },
      );
      expect(await connection.getBalance(nonceAccount.publicKey)).to.eq(0);
      const withdrawBalance = await connection.getBalance(
        withdrawAccount.publicKey,
      );
      expect(withdrawBalance).to.eq(minimumAmount);
    }).timeout(10 * 1000);

    it('live withSeed actions', async () => {
      const connection = new Connection(url, 'confirmed');
      const baseAccount = Keypair.generate();
      await helpers.airdrop({
        connection,
        address: baseAccount.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });
      const basePubkey = baseAccount.publicKey;
      const seed = 'hi there';
      const programId = Keypair.generate().publicKey;
      const createAccountWithSeedAddress = await PublicKey.createWithSeed(
        basePubkey,
        seed,
        programId,
      );
      const space = 0;

      const minimumAmount = await connection.getMinimumBalanceForRentExemption(
        space,
      );

      // Test CreateAccountWithSeed
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
        {preflightCommitment: 'confirmed'},
      );
      const createAccountWithSeedBalance = await connection.getBalance(
        createAccountWithSeedAddress,
      );
      expect(createAccountWithSeedBalance).to.eq(minimumAmount);

      // Test CreateAccountWithSeed where fromPubkey != basePubkey
      const uniqueFromAccount = Keypair.generate();
      const newBaseAccount = Keypair.generate();
      const createAccountWithSeedAddress2 = await PublicKey.createWithSeed(
        newBaseAccount.publicKey,
        seed,
        programId,
      );
      await helpers.airdrop({
        connection,
        address: uniqueFromAccount.publicKey,
        amount: 2 * LAMPORTS_PER_SOL,
      });
      const createAccountWithSeedParams2 = {
        fromPubkey: uniqueFromAccount.publicKey,
        newAccountPubkey: createAccountWithSeedAddress2,
        basePubkey: newBaseAccount.publicKey,
        seed,
        lamports: minimumAmount,
        space,
        programId,
      };
      const createAccountWithSeedTransaction2 = new Transaction().add(
        SystemProgram.createAccountWithSeed(createAccountWithSeedParams2),
      );
      await sendAndConfirmTransaction(
        connection,
        createAccountWithSeedTransaction2,
        [uniqueFromAccount, newBaseAccount],
        {preflightCommitment: 'confirmed'},
      );
      const createAccountWithSeedBalance2 = await connection.getBalance(
        createAccountWithSeedAddress2,
      );
      expect(createAccountWithSeedBalance2).to.eq(minimumAmount);

      // Transfer to a derived address to prep for TransferWithSeed
      const programId2 = Keypair.generate().publicKey;
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
        {preflightCommitment: 'confirmed'},
      );
      let transferWithSeedAddressBalance = await connection.getBalance(
        transferWithSeedAddress,
      );
      expect(transferWithSeedAddressBalance).to.eq(3 * minimumAmount);

      // Test TransferWithSeed
      const programId3 = Keypair.generate();
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
        {preflightCommitment: 'confirmed'},
      );
      const toBalance = await connection.getBalance(toPubkey);
      expect(toBalance).to.eq(2 * minimumAmount);
      transferWithSeedAddressBalance = await connection.getBalance(
        createAccountWithSeedAddress,
      );
      expect(transferWithSeedAddressBalance).to.eq(minimumAmount);

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
        {preflightCommitment: 'confirmed'},
      );
      let account = await connection.getAccountInfo(toPubkey);
      if (account === null) {
        expect(account).not.to.be.null;
        return;
      }
      expect(account.data).to.have.length(10);

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
        {preflightCommitment: 'confirmed'},
      );
      account = await connection.getAccountInfo(toPubkey);
      if (account === null) {
        expect(account).not.to.be.null;
        return;
      }
      expect(account.owner).to.eql(programId3.publicKey);
    }).timeout(10 * 1000);
  }
});
