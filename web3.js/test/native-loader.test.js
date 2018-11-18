// @flow

import {
  Connection,
  NativeLoader,
  Transaction,
  sendAndConfirmTransaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithTokens} from './new-account-with-tokens';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(15000);
}

test('load native program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const from = await newAccountWithTokens(connection);
  const programId = await NativeLoader.load(connection, from, 'noop');
  const transaction = new Transaction().add({
    keys: [from.publicKey],
    programId,
  });

  await sendAndConfirmTransaction(connection, transaction, from);
});
