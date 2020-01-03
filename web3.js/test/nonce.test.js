// @flow

import bs58 from 'bs58';

import {Account, Connection, SystemProgram} from '../src';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

test('create and query nonce account', async () => {
  const from = new Account();
  const nonceAccount = new Account();
  const connection = new Connection(url, 'recent');

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [68, {commitment: 'recent'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    SystemProgram.nonceSpace,
    'recent',
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [
        from.publicKey.toBase58(),
        minimumAmount * 2,
        {commitment: 'recent'},
      ],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  await connection.requestAirdrop(from.publicKey, minimumAmount * 2);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [from.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: minimumAmount * 2,
    },
  ]);

  const balance = await connection.getBalance(from.publicKey);
  expect(balance).toBe(minimumAmount * 2);

  mockGetRecentBlockhash('recent');
  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
    },
    {
      error: null,
      result:
        '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const transaction = SystemProgram.createNonceAccount(
    from.publicKey,
    nonceAccount.publicKey,
    from.publicKey,
    minimumAmount,
  );
  await connection.sendTransaction(transaction, from, nonceAccount);

  const expectedData = Buffer.alloc(68);
  expectedData.writeInt32LE(1, 0);
  from.publicKey.toBuffer().copy(expectedData, 4);
  const mockNonce = new Account();
  mockNonce.publicKey.toBuffer().copy(expectedData, 36);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [nonceAccount.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        owner: [
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
          0,
        ],
        lamports: minimumAmount,
        data: [...expectedData],
        executable: false,
      },
    },
  ]);
  //
  const nonceAccountData = await connection.getNonce(
    nonceAccount.publicKey,
    'recent',
  );
  expect(nonceAccountData.authorizedPubkey).toEqual(from.publicKey);
  expect(bs58.decode(nonceAccountData.nonce).length).toBeGreaterThan(30);
});
