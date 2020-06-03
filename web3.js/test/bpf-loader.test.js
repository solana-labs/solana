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
  await BpfLoader.load(connection, from, program, data);
  const transaction = new Transaction().add({
    keys: [{pubkey: from.publicKey, isSigner: true, isWritable: true}],
    programId: program.publicKey,
  });
  await sendAndConfirmTransaction(connection, transaction, [from], {
    confirmations: 1,
    skipPreflight: true,
  });
});

test('load BPF Rust program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const data = await fs.readFile(
    'test/fixtures/noop-rust/solana_bpf_rust_noop.so',
  );

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
  await BpfLoader.load(connection, from, program, data);
  const transaction = new Transaction().add({
    keys: [{pubkey: from.publicKey, isSigner: true, isWritable: true}],
    programId: program.publicKey,
  });
  await sendAndConfirmTransaction(connection, transaction, [from], {
    confirmations: 1,
    skipPreflight: true,
  });
});
