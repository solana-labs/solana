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
  jest.setTimeout(120000);
}

const NUM_RETRIES = 100; /* allow some number of retries */

test('load BPF C program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const data = await fs.readFile('test/fixtures/noop-c/noop.so');

  const connection = new Connection(url, 'recent');
  const {feeCalculator} = await connection.getRecentBlockhash();
  const fees =
    feeCalculator.lamportsPerSignature *
    (BpfLoader.getMinNumSignatures(data.length) + NUM_RETRIES);
  const balanceNeeded = await connection.getMinimumBalanceForRentExemption(
    data.length,
  );
  const from = await newAccountWithLamports(connection, fees + balanceNeeded);

  const program = new Account();
  await BpfLoader.load(connection, from, program, data, BPF_LOADER_PROGRAM_ID);
  const transaction = new Transaction().add({
    keys: [{pubkey: from.publicKey, isSigner: true, isWritable: true}],
    programId: program.publicKey,
  });
  await sendAndConfirmTransaction(connection, transaction, [from], {
    commitment: 'single',
    skipPreflight: true,
  });
});

describe('load BPF Rust program', () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');

  let program: Account;
  let signature: string;
  let payerAccount: Account;

  beforeAll(async () => {
    const data = await fs.readFile(
      'test/fixtures/noop-rust/solana_bpf_rust_noop.so',
    );

    const {feeCalculator} = await connection.getRecentBlockhash();
    const fees =
      feeCalculator.lamportsPerSignature *
      (BpfLoader.getMinNumSignatures(data.length) + NUM_RETRIES);
    const balanceNeeded = await connection.getMinimumBalanceForRentExemption(
      data.length,
    );

    payerAccount = await newAccountWithLamports(
      connection,
      fees + balanceNeeded,
    );

    program = new Account();
    await BpfLoader.load(
      connection,
      payerAccount,
      program,
      data,
      BPF_LOADER_PROGRAM_ID,
    );

    const transaction = new Transaction().add({
      keys: [
        {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
      ],
      programId: program.publicKey,
    });

    signature = await sendAndConfirmTransaction(
      connection,
      transaction,
      [payerAccount],
      {
        skipPreflight: true,
      },
    );
  });

  test('get confirmed transaction', async () => {
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
    expect(logs[0]).toEqual(`Call BPF program ${program.publicKey.toBase58()}`);
    expect(logs[logs.length - 1]).toEqual(
      `BPF program ${program.publicKey.toBase58()} success`,
    );
  });

  test('simulate transaction without signature verification', async () => {
    const simulatedTransaction = new Transaction().add({
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
    expect(logs[0]).toEqual(`Call BPF program ${program.publicKey.toBase58()}`);
    expect(logs[logs.length - 1]).toEqual(
      `BPF program ${program.publicKey.toBase58()} success`,
    );
  });

  test('simulate transaction with bad programId', async () => {
    const simulatedTransaction = new Transaction().add({
      keys: [
        {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
      ],
      programId: new Account().publicKey,
    });

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

  test('simulate transaction with bad signer', async () => {
    const simulatedTransaction = new Transaction().add({
      keys: [
        {pubkey: payerAccount.publicKey, isSigner: true, isWritable: true},
      ],
      programId: program.publicKey,
    });

    const {err, logs} = (
      await connection.simulateTransaction(simulatedTransaction, [program])
    ).value;
    expect(err).toEqual('SignatureFailure');
    expect(logs).toBeNull();
  });
});
