// @flow

import type {TransactionSignature} from '../../src/transaction';
import {mockRpcSocket} from '../__mocks__/rpc-websockets';

export function mockConfirmTransaction(signature: TransactionSignature) {
  mockRpcSocket.push([
    {
      method: 'signatureSubscribe',
      params: [signature, {commitment: 'single'}],
    },
    {
      context: {
        slot: 11,
      },
      value: {err: null},
    },
  ]);
}
