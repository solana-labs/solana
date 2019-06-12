// @flow

import {Account} from '../../src';
import {url} from '../url';
import {mockRpc} from '../__mocks__/node-fetch';

export function mockGetRecentBlockhash() {
  const recentBlockhash = new Account();

  mockRpc.push([
    url,
    {
      method: 'getRecentBlockhash',
      params: [],
    },
    {
      error: null,
      result: [
        recentBlockhash.publicKey.toBase58(),
        {
          lamportsPerSignature: 42,
        },
      ],
    },
  ]);
}
