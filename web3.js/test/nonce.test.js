// @flow

import bs58 from 'bs58';

import {Account, Connection, SystemProgram} from '../src';
import {NONCE_ACCOUNT_LENGTH} from '../src/nonce-account';
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
      params: [NONCE_ACCOUNT_LENGTH, {commitment: 'recent'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    NONCE_ACCOUNT_LENGTH,
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
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount * 2,
      },
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

  const transaction = SystemProgram.createNonceAccount({
    fromPubkey: from.publicKey,
    noncePubkey: nonceAccount.publicKey,
    authorizedPubkey: from.publicKey,
    lamports: minimumAmount,
  });
  await connection.sendTransaction(transaction, from, nonceAccount);

  const expectedData = Buffer.alloc(NONCE_ACCOUNT_LENGTH);
  expectedData.writeInt32LE(0, 0); // Version, 4 bytes
  expectedData.writeInt32LE(1, 4); // State, 4 bytes
  from.publicKey.toBuffer().copy(expectedData, 8); // authorizedPubkey, 32 bytes
  const mockNonce = new Account();
  mockNonce.publicKey.toBuffer().copy(expectedData, 40); // Hash, 32 bytes
  expectedData.writeUInt16LE(5000, 72); // feeCalculator, 8 bytes

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [nonceAccount.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: {
          owner: '11111111111111111111111111111111',
          lamports: minimumAmount,
          data: bs58.encode(expectedData),
          executable: false,
        },
      },
    },
  ]);
  //
  const nonceAccountData = await connection.getNonce(
    nonceAccount.publicKey,
    'recent',
  );
  if (nonceAccountData === null) {
    expect(nonceAccountData).not.toBeNull();
    return;
  }
  expect(nonceAccountData.authorizedPubkey).toEqual(from.publicKey);
  expect(bs58.decode(nonceAccountData.nonce).length).toBeGreaterThan(30);
});
