import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Keypair,
  Connection,
  LAMPORTS_PER_SOL,
  Transaction,
  ComputeBudgetProgram,
  ComputeBudgetInstruction,
  sendAndConfirmTransaction,
} from '../src';
import {helpers} from './mocks/rpc-http';
import {url} from './url';

use(chaiAsPromised);

describe('ComputeBudgetProgram', () => {
  it('requestUnits', () => {
    const params = {
      units: 150000,
      additionalFee: LAMPORTS_PER_SOL,
    };
    const ix = ComputeBudgetProgram.requestUnits(params);
    const decodedParams = ComputeBudgetInstruction.decodeRequestUnits(ix);
    expect(params).to.eql(decodedParams);
    expect(ComputeBudgetInstruction.decodeInstructionType(ix)).to.eq(
      'RequestUnits',
    );
  });

  it('requestHeapFrame', () => {
    const params = {
      bytes: 33 * 1024,
    };
    const ix = ComputeBudgetProgram.requestHeapFrame(params);
    const decodedParams = ComputeBudgetInstruction.decodeRequestHeapFrame(ix);
    expect(decodedParams).to.eql(params);
    expect(ComputeBudgetInstruction.decodeInstructionType(ix)).to.eq(
      'RequestHeapFrame',
    );
  });

  it('setComputeUnitLimit', () => {
    const params = {
      units: 50_000,
    };
    const ix = ComputeBudgetProgram.setComputeUnitLimit(params);
    const decodedParams =
      ComputeBudgetInstruction.decodeSetComputeUnitLimit(ix);
    expect(decodedParams).to.eql(params);
    expect(ComputeBudgetInstruction.decodeInstructionType(ix)).to.eq(
      'SetComputeUnitLimit',
    );
  });

  it('setComputeUnitPrice', () => {
    const params = {
      microLamports: 100_000,
    };
    const ix = ComputeBudgetProgram.setComputeUnitPrice(params);
    const expectedParams = {
      ...params,
      microLamports: BigInt(params.microLamports),
    };
    const decodedParams =
      ComputeBudgetInstruction.decodeSetComputeUnitPrice(ix);
    expect(decodedParams).to.eql(expectedParams);
    expect(ComputeBudgetInstruction.decodeInstructionType(ix)).to.eq(
      'SetComputeUnitPrice',
    );
  });

  if (process.env.TEST_LIVE) {
    it('send live request units ix', async () => {
      const connection = new Connection(url, 'confirmed');
      const FEE_AMOUNT = LAMPORTS_PER_SOL;
      const STARTING_AMOUNT = 2 * LAMPORTS_PER_SOL;
      const baseAccount = Keypair.generate();
      const basePubkey = baseAccount.publicKey;
      await helpers.airdrop({
        connection,
        address: basePubkey,
        amount: STARTING_AMOUNT,
      });

      const additionalFeeTooHighTransaction = new Transaction().add(
        ComputeBudgetProgram.requestUnits({
          units: 150_000,
          additionalFee: STARTING_AMOUNT,
        }),
      );

      await expect(
        sendAndConfirmTransaction(
          connection,
          additionalFeeTooHighTransaction,
          [baseAccount],
          {preflightCommitment: 'confirmed'},
        ),
      ).to.be.rejected;

      const validAdditionalFeeTransaction = new Transaction().add(
        ComputeBudgetProgram.requestUnits({
          units: 150_000,
          additionalFee: FEE_AMOUNT,
        }),
      );
      await sendAndConfirmTransaction(
        connection,
        validAdditionalFeeTransaction,
        [baseAccount],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(baseAccount.publicKey)).to.be.at.most(
        STARTING_AMOUNT - FEE_AMOUNT,
      );
    });

    it('send live request heap ix', async () => {
      const connection = new Connection(url, 'confirmed');
      const STARTING_AMOUNT = 2 * LAMPORTS_PER_SOL;
      const baseAccount = Keypair.generate();
      const basePubkey = baseAccount.publicKey;
      await helpers.airdrop({
        connection,
        address: basePubkey,
        amount: STARTING_AMOUNT,
      });

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
    });

    it('send live compute unit ixs', async () => {
      const connection = new Connection(url, 'confirmed');
      const FEE_AMOUNT = LAMPORTS_PER_SOL;
      const STARTING_AMOUNT = 2 * LAMPORTS_PER_SOL;
      const baseAccount = Keypair.generate();
      const basePubkey = baseAccount.publicKey;
      await helpers.airdrop({
        connection,
        address: basePubkey,
        amount: STARTING_AMOUNT,
      });

      // lamport fee = 2B * 1M / 1M = 2 SOL
      const prioritizationFeeTooHighTransaction = new Transaction()
        .add(
          ComputeBudgetProgram.setComputeUnitPrice({
            microLamports: 2_000_000_000,
          }),
        )
        .add(
          ComputeBudgetProgram.setComputeUnitLimit({
            units: 1_000_000,
          }),
        );

      await expect(
        sendAndConfirmTransaction(
          connection,
          prioritizationFeeTooHighTransaction,
          [baseAccount],
          {preflightCommitment: 'confirmed'},
        ),
      ).to.be.rejected;

      // lamport fee = 1B * 1M / 1M = 1 SOL
      const validPrioritizationFeeTransaction = new Transaction()
        .add(
          ComputeBudgetProgram.setComputeUnitPrice({
            microLamports: 1_000_000_000,
          }),
        )
        .add(
          ComputeBudgetProgram.setComputeUnitLimit({
            units: 1_000_000,
          }),
        );
      await sendAndConfirmTransaction(
        connection,
        validPrioritizationFeeTransaction,
        [baseAccount],
        {preflightCommitment: 'confirmed'},
      );
      expect(await connection.getBalance(baseAccount.publicKey)).to.be.at.most(
        STARTING_AMOUNT - FEE_AMOUNT,
      );
    });
  }
});
