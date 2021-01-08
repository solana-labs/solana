// @flow

import fs from 'mz/fs';

import {
  Connection,
  BpfLoader,
  Transaction,
  sendAndConfirmTransaction,
  Account,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithLamports} from './new-account-with-lamports';
import {BPF_LOADER_PROGRAM_ID} from '../src/bpf-loader';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(240000);
}

test('load BPF C program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const data = await fs.readFile('test/fixtures/noop-c/noop.so');

  const connection = new Connection(url, 'singleGossip');
  const {feeCalculator} = await connection.getRecentBlockhash();
  const fees =
    feeCalculator.lamportsPerSignature *
    BpfLoader.getMinNumSignatures(data.length);
  const payerBalance = await connection.getMinimumBalanceForRentExemption(0);
  const executableBalance = await connection.getMinimumBalanceForRentExemption(
    data.length,
  );
  const from = await newAccountWithLamports(
    connection,
    payerBalance + fees + executableBalance,
  );

  const program = new Account();
  await BpfLoader.load(connection, from, program, data, BPF_LOADER_PROGRAM_ID);

  // Check that program loading costed exactly `fees + executableBalance`
  const fromBalance = await connection.getBalance(from.publicKey);
  expect(fromBalance).toEqual(payerBalance);

  const transaction = new Transaction().add({
    keys: [{pubkey: from.publicKey, isSigner: true, isWritable: true}],
    programId: program.publicKey,
  });
  await sendAndConfirmTransaction(connection, transaction, [from], {
    commitment: 'singleGossip',
    preflightCommitment: 'singleGossip',
  });
});

describe('load BPF Rust program', () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'singleGossip');

  let program: Account;
  let payerAccount: Account;
  let programData: Buffer;

  beforeAll(async () => {
    programData = await fs.readFile(
      'test/fixtures/noop-rust/solana_bpf_rust_noop.so',
    );

    const {feeCalculator} = await connection.getRecentBlockhash();
    const fees =
      feeCalculator.lamportsPerSignature *
      BpfLoader.getMinNumSignatures(programData.length);
    const payerBalance = await connection.getMinimumBalanceForRentExemption(0);
    const executableBalance = await connection.getMinimumBalanceForRentExemption(
      programData.length,
    );

    payerAccount = await newAccountWithLamports(
      connection,
      payerBalance + fees + executableBalance,
    );

    // Create program account with low balance
    program = await newAccountWithLamports(connection, executableBalance - 1);

    // First load will fail part way due to lack of funds
    const insufficientPayerAccount = await newAccountWithLamports(
      connection,
      2 * feeCalculator.lamportsPerSignature * 8,
    );

    const failedLoad = BpfLoader.load(
      connection,
      insufficientPayerAccount,
      program,
      programData,
      BPF_LOADER_PROGRAM_ID,
    );
    await expect(failedLoad).rejects.toThrow();

    // Second load will succeed
    await BpfLoader.load(
      connection,
      payerAccount,
      program,
      programData,
      BPF_LOADER_PROGRAM_ID,
    );
  });

  test('get confirmed transaction', async () => {
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

    const parsedTx = await connection.getParsedConfirmedTransaction(signature);
    if (parsedTx === null) {
      expect(parsedTx).not.toBeNull();
      return;
    }
    const {signatures, message} = parsedTx.transaction;
    expect(signatures[0]).toEqual(signature);
    const ix = message.instructions[0];
    if (ix.parsed) {
      expect('parsed' in ix).toBe(false);
    } else {
      expect(ix.programId.equals(program.publicKey)).toBe(true);
      expect(ix.data).toEqual('');
    }
  });

  test('simulate transaction', async () => {
    const simulatedTransaction = new Transaction().add({
      keys: [
        {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
      ],
      programId: program.publicKey,
    });

    const {err, logs} = (
      await connection.simulateTransaction(simulatedTransaction, [payerAccount])
    ).value;
    expect(err).toBeNull();

    if (logs === null) {
      expect(logs).not.toBeNull();
      return;
    }

    expect(logs.length).toBeGreaterThanOrEqual(2);
    expect(logs[0]).toEqual(
      `Program ${program.publicKey.toBase58()} invoke [1]`,
    );
    expect(logs[logs.length - 1]).toEqual(
      `Program ${program.publicKey.toBase58()} success`,
    );
  });

  test('deprecated - simulate transaction without signature verification', async () => {
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
    expect(err).toBeNull();

    if (logs === null) {
      expect(logs).not.toBeNull();
      return;
    }

    expect(logs.length).toBeGreaterThanOrEqual(2);
    expect(logs[0]).toEqual(
      `Program ${program.publicKey.toBase58()} invoke [1]`,
    );
    expect(logs[logs.length - 1]).toEqual(
      `Program ${program.publicKey.toBase58()} success`,
    );
  });

  test('simulate transaction without signature verification', async () => {
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
    expect(err).toBeNull();

    if (logs === null) {
      expect(logs).not.toBeNull();
      return;
    }

    expect(logs.length).toBeGreaterThanOrEqual(2);
    expect(logs[0]).toEqual(
      `Program ${program.publicKey.toBase58()} invoke [1]`,
    );
    expect(logs[logs.length - 1]).toEqual(
      `Program ${program.publicKey.toBase58()} success`,
    );
  });

  test('simulate transaction with bad programId', async () => {
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
    expect(err).toEqual('ProgramAccountNotFound');

    if (logs === null) {
      expect(logs).not.toBeNull();
      return;
    }

    expect(logs.length).toEqual(0);
  });

  test('reload program', async () => {
    expect(
      await BpfLoader.load(
        connection,
        payerAccount,
        program,
        programData,
        BPF_LOADER_PROGRAM_ID,
      ),
    ).toBe(false);
  });
});
