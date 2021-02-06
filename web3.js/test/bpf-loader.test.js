// @flow

import fs from 'mz/fs';
import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Connection,
  BpfLoader,
  Transaction,
  sendAndConfirmTransaction,
  Account,
} from '../src';
import {url} from './url';
import {BPF_LOADER_PROGRAM_ID} from '../src/bpf-loader';
import {helpers} from './mocks/rpc-http';

use(chaiAsPromised);

if (process.env.TEST_LIVE) {
  describe('BPF Loader', () => {
    it('load BPF C program', async () => {
      const data = await fs.readFile('test/fixtures/noop-c/noop.so');

      const connection = new Connection(url, 'singleGossip');
      const {feeCalculator} = await connection.getRecentBlockhash();
      const fees =
        feeCalculator.lamportsPerSignature *
        BpfLoader.getMinNumSignatures(data.length);
      const payerBalance = await connection.getMinimumBalanceForRentExemption(
        0,
      );
      const executableBalance = await connection.getMinimumBalanceForRentExemption(
        data.length,
      );

      const from = new Account();
      await helpers.airdrop({
        connection,
        address: from.publicKey,
        amount: payerBalance + fees + executableBalance,
      });

      const program = new Account();
      await BpfLoader.load(
        connection,
        from,
        program,
        data,
        BPF_LOADER_PROGRAM_ID,
      );

      // Check that program loading costed exactly `fees + executableBalance`
      const fromBalance = await connection.getBalance(from.publicKey);
      expect(fromBalance).to.eq(payerBalance);

      const transaction = new Transaction().add({
        keys: [{pubkey: from.publicKey, isSigner: true, isWritable: true}],
        programId: program.publicKey,
      });
      await sendAndConfirmTransaction(connection, transaction, [from], {
        commitment: 'singleGossip',
        preflightCommitment: 'singleGossip',
      });
    }).timeout(5000);

    describe('load BPF Rust program', () => {
      const connection = new Connection(url, 'singleGossip');

      let program = new Account();
      let payerAccount = new Account();
      let programData: Buffer;

      before(async function () {
        this.timeout(60_000);
        programData = await fs.readFile(
          'test/fixtures/noop-rust/solana_bpf_rust_noop.so',
        );

        const {feeCalculator} = await connection.getRecentBlockhash();
        const fees =
          feeCalculator.lamportsPerSignature *
          BpfLoader.getMinNumSignatures(programData.length);
        const payerBalance = await connection.getMinimumBalanceForRentExemption(
          0,
        );
        const executableBalance = await connection.getMinimumBalanceForRentExemption(
          programData.length,
        );

        await helpers.airdrop({
          connection,
          address: payerAccount.publicKey,
          amount: payerBalance + fees + executableBalance,
        });

        // Create program account with low balance
        await helpers.airdrop({
          connection,
          address: program.publicKey,
          amount: executableBalance - 1,
        });

        // First load will fail part way due to lack of funds
        const insufficientPayerAccount = new Account();
        await helpers.airdrop({
          connection,
          address: insufficientPayerAccount.publicKey,
          amount: 2 * feeCalculator.lamportsPerSignature * 8,
        });

        const failedLoad = BpfLoader.load(
          connection,
          insufficientPayerAccount,
          program,
          programData,
          BPF_LOADER_PROGRAM_ID,
        );
        await expect(failedLoad).to.be.rejected;

        // Second load will succeed
        await BpfLoader.load(
          connection,
          payerAccount,
          program,
          programData,
          BPF_LOADER_PROGRAM_ID,
        );
      });

      it('get confirmed transaction', async () => {
        const transaction = new Transaction().add({
          keys: [
            {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
          ],
          programId: program.publicKey,
        });

        const signature = await sendAndConfirmTransaction(
          connection,
          transaction,
          [payerAccount],
          {
            commitment: 'max', // `getParsedConfirmedTransaction` requires max commitment
            preflightCommitment: connection.commitment || 'max',
          },
        );

        const parsedTx = await connection.getParsedConfirmedTransaction(
          signature,
        );
        if (parsedTx === null) {
          expect(parsedTx).not.to.be.null;
          return;
        }
        const {signatures, message} = parsedTx.transaction;
        expect(signatures[0]).to.eq(signature);
        const ix = message.instructions[0];
        if (ix.parsed) {
          expect('parsed' in ix).to.eq(false);
        } else {
          expect(ix.programId).to.eql(program.publicKey);
          expect(ix.data).to.eq('');
        }
      }).timeout(30000);

      it('simulate transaction', async () => {
        const simulatedTransaction = new Transaction().add({
          keys: [
            {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
          ],
          programId: program.publicKey,
        });

        const {err, logs} = (
          await connection.simulateTransaction(simulatedTransaction, [
            payerAccount,
          ])
        ).value;
        expect(err).to.be.null;

        if (logs === null) {
          expect(logs).not.to.be.null;
          return;
        }

        expect(logs.length).to.be.at.least(2);
        expect(logs[0]).to.eq(
          `Program ${program.publicKey.toBase58()} invoke [1]`,
        );
        expect(logs[logs.length - 1]).to.eq(
          `Program ${program.publicKey.toBase58()} success`,
        );
      });

      it('deprecated - simulate transaction without signature verification', async () => {
        const simulatedTransaction = new Transaction().add({
          keys: [
            {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
          ],
          programId: program.publicKey,
        });

        simulatedTransaction.setSigners(payerAccount.publicKey);
        const {err, logs} = (
          await connection.simulateTransaction(simulatedTransaction)
        ).value;
        expect(err).to.be.null;

        if (logs === null) {
          expect(logs).not.to.be.null;
          return;
        }

        expect(logs.length).to.be.at.least(2);
        expect(logs[0]).to.eq(
          `Program ${program.publicKey.toBase58()} invoke [1]`,
        );
        expect(logs[logs.length - 1]).to.eq(
          `Program ${program.publicKey.toBase58()} success`,
        );
      });

      it('simulate transaction without signature verification', async () => {
        const simulatedTransaction = new Transaction({
          feePayer: payerAccount.publicKey,
        }).add({
          keys: [
            {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
          ],
          programId: program.publicKey,
        });

        const {err, logs} = (
          await connection.simulateTransaction(simulatedTransaction)
        ).value;
        expect(err).to.be.null;

        if (logs === null) {
          expect(logs).not.to.be.null;
          return;
        }

        expect(logs.length).to.be.at.least(2);
        expect(logs[0]).to.eq(
          `Program ${program.publicKey.toBase58()} invoke [1]`,
        );
        expect(logs[logs.length - 1]).to.eq(
          `Program ${program.publicKey.toBase58()} success`,
        );
      });

      it('simulate transaction with bad programId', async () => {
        const simulatedTransaction = new Transaction().add({
          keys: [
            {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
          ],
          programId: new Account().publicKey,
        });

        simulatedTransaction.setSigners(payerAccount.publicKey);
        const {err, logs} = (
          await connection.simulateTransaction(simulatedTransaction)
        ).value;
        expect(err).to.eq('ProgramAccountNotFound');

        if (logs === null) {
          expect(logs).not.to.be.null;
          return;
        }

        expect(logs).to.have.length(0);
      });

      it('reload program', async () => {
        expect(
          await BpfLoader.load(
            connection,
            payerAccount,
            program,
            programData,
            BPF_LOADER_PROGRAM_ID,
          ),
        ).to.eq(false);
      });
    });
  });
}
