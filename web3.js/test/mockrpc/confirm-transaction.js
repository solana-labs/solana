// @flow

import type {TransactionSignature} from '../../src/transaction';
import {url} from '../url';
import {mockRpc} from '../__mocks__/node-fetch';

export function mockConfirmTransaction(signature: TransactionSignature) {
  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [[signature]],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: [
          {
            slot: 0,
            confirmations: null,
            status: {Ok: null},
            err: null,
          },
        ],
      },
    },
  ]);
}
