// @flow

import {
  Account,
} from '../../src';
import {url} from '../url';
import {mockRpc} from '../__mocks__/node-fetch';

export function mockGetLastId() {
  const lastId = new Account();

  mockRpc.push([
    url,
    {
      method: 'getLastId',
      params: [],
    },
    {
      error: null,
      result: lastId.publicKey.toBase58(),
    }
  ]);
}
