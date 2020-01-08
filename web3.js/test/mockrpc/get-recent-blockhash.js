// @flow

import {Account} from '../../src';
import type {Commitment} from '../../src/connection';
import {url} from '../url';
import {mockRpc} from '../__mocks__/node-fetch';

export function mockGetRecentBlockhash(commitment: ?Commitment) {
  const recentBlockhash = new Account();
  const params = [];
  if (commitment) {
    params.push({commitment});
  }

  mockRpc.push([
    url,
    {
      method: 'getRecentBlockhash',
      params,
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: [
          recentBlockhash.publicKey.toBase58(),
          {
            lamportsPerSignature: 42,
            burnPercent: 50,
            maxLamportsPerSignature: 42,
            minLamportsPerSignature: 42,
            targetLamportsPerSignature: 42,
            targetSignaturesPerSlot: 42,
          },
        ],
      },
    },
  ]);
}
