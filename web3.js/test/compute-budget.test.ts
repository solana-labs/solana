import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Keypair,
  Connection,
  LAMPORTS_PER_SOL,
  Transaction,
  ComputeBudgetProgram,
  ComputeBudgetInstruction,
  PublicKey,
  SystemProgram,
  sendAndConfirmTransaction,
} from '../src';
import {helpers} from './mocks/rpc-http';
import {url} from './url';

use(chaiAsPromised);

describe('ComputeBudgetProgram', () => {
  it('requestUnits', () => {
    const params = {
      units: 150000,
      additionalFee: 0,
    };
    const transaction = new Transaction().add(
      ComputeBudgetProgram.requestUnits(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [computeBudgetInstruction] = transaction.instructions;
    expect(params).to.eql(
      ComputeBudgetInstruction.decodeRequestUnits(computeBudgetInstruction),
    );
  });

  it('requestHeapFrame', () => {
    const params = {
      bytes: 33 * 1024,
    };
    const transaction = new Transaction().add(
      ComputeBudgetProgram.requestHeapFrame(params),
    );
    expect(transaction.instructions).to.have.length(1);
    const [computeBudgetInstruction] = transaction.instructions;
    expect(params).to.eql(
      ComputeBudgetInstruction.decodeRequestHeapFrame(computeBudgetInstruction),
    );
  });

  it('ComputeBudgetInstruction', () => {
    const requestUnits = ComputeBudgetProgram.requestUnits({
      units: 150000,
      additionalFee: 0,
    });
    const requestHeapFrame = ComputeBudgetProgram.requestHeapFrame({
      bytes: 33 * 1024,
    });

    const requestUnitsTransaction = new Transaction().add(requestUnits);
    expect(requestUnitsTransaction.instructions).to.have.length(1);
    const requestUnitsTransactionType =
      ComputeBudgetInstruction.decodeInstructionType(
        requestUnitsTransaction.instructions[0],
      );
    expect(requestUnitsTransactionType).to.eq('RequestUnits');

    const requestHeapFrameTransaction = new Transaction().add(requestHeapFrame);
    expect(requestHeapFrameTransaction.instructions).to.have.length(1);
    const requestHeapFrameTransactionType =
      ComputeBudgetInstruction.decodeInstructionType(
        requestHeapFrameTransaction.instructions[0],
      );
    expect(requestHeapFrameTransactionType).to.eq('RequestHeapFrame');
  });

  if (process.env.TEST_LIVE) {
    const STARTING_AMOUNT = 2 * LAMPORTS_PER_SOL;
    const FEE_AMOUNT = LAMPORTS_PER_SOL;
    it('live compute budget actions', async () => {
      const connection = new Connection(url, 'confirmed');

      const baseAccount = Keypair.generate();
      const basePubkey = baseAccount.publicKey;
      await helpers.airdrop({
        connection,
        address: basePubkey,
        amount: STARTING_AMOUNT,
      });

      expect(await connection.getBalance(baseAccount.publicKey)).to.eq(
        STARTING_AMOUNT,
      );

      const seed = 'hi there';
      const programId = Keypair.generate().publicKey;
      const createAccountWithSeedAddress = await PublicKey.createWithSeed(
        basePubkey,
        seed,
        programId,
      );
      const space = 0;

      let minimumAmount = await connection.getMinimumBalanceForRentExemption(
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

      const createAccountFeeTooHighTransaction = new Transaction().add(
        ComputeBudgetProgram.requestUnits({
          units: 2,
          additionalFee: 2 * FEE_AMOUNT,
        }),
        SystemProgram.createAccountWithSeed(createAccountWithSeedParams),
      );
      await expect(
        sendAndConfirmTransaction(
          connection,
          createAccountFeeTooHighTransaction,
          [baseAccount],
          {preflightCommitment: 'confirmed'},
        ),
      ).to.be.rejected;

      expect(await connection.getBalance(baseAccount.publicKey)).to.eq(
        STARTING_AMOUNT,
      );

      const createAccountFeeTransaction = new Transaction().add(
        ComputeBudgetProgram.requestUnits({
          units: 2,
          additionalFee: FEE_AMOUNT,
        }),
        SystemProgram.createAccountWithSeed(createAccountWithSeedParams),
      );
      await sendAndConfirmTransaction(
        connection,
        createAccountFeeTransaction,
        [baseAccount],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(baseAccount.publicKey)).to.be.at.most(
        STARTING_AMOUNT - FEE_AMOUNT - minimumAmount,
      );

      async function expectRequestHeapFailure(bytes: number) {
        const requestHeapFrameTransaction = new Transaction().add(
          ComputeBudgetProgram.requestHeapFrame({bytes}),
        );
        await expect(
          sendAndConfirmTransaction(
            connection,
            requestHeapFrameTransaction,
            [baseAccount],
            {preflightCommitment: 'confirmed'},
          ),
        ).to.be.rejected;
      }
      const NOT_MULTIPLE_OF_1024 = 33 * 1024 + 1;
      const BELOW_MIN = 1024;
      const ABOVE_MAX = 257 * 1024;
      await expectRequestHeapFailure(NOT_MULTIPLE_OF_1024);
      await expectRequestHeapFailure(BELOW_MIN);
      await expectRequestHeapFailure(ABOVE_MAX);

      const VALID_BYTES = 33 * 1024;
      const requestHeapFrameTransaction = new Transaction().add(
        ComputeBudgetProgram.requestHeapFrame({bytes: VALID_BYTES}),
      );
      await sendAndConfirmTransaction(
        connection,
        requestHeapFrameTransaction,
        [baseAccount],
        {preflightCommitment: 'confirmed'},
      );
    }).timeout(10 * 1000);
  }
});
