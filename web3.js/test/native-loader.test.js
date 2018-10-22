// @flow

import {
  Connection,
  NativeLoader,
  Transaction,
} from '../src';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {newAccountWithTokens} from './new-account-with-tokens';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(10000);
}


test('load noop program', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const from = await newAccountWithTokens(connection);

  const noopProgramId = await NativeLoader.load(connection, from, 'noop');
  const noopTransaction = new Transaction({
    fee: 0,
    keys: [from.publicKey],
    programId: noopProgramId,
  });
  const signature = await connection.sendTransaction(from, noopTransaction);
  await expect(connection.confirmTransaction(signature)).resolves.toBe(true);
});

