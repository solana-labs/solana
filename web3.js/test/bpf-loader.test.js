// @flow

import {
  Connection,
  BpfLoader,
  Transaction,
  sendAndConfirmTransaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithTokens} from './new-account-with-tokens';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(10000);
}

test('load BPF program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const from = await newAccountWithTokens(connection);
  const programId = await BpfLoader.load(connection, from, 'test/bin/noop_c.o');
  const transaction = new Transaction().add({
    keys: [from.publicKey],
    programId,
  });
  await sendAndConfirmTransaction(connection, from, transaction);
});

