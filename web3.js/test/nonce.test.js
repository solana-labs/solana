// @flow

import bs58 from 'bs58';

import {Account, Connection, SystemProgram, PublicKey} from '../src';
import {NONCE_ACCOUNT_LENGTH} from '../src/nonce-account';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';
import {mockConfirmTransaction} from './mockrpc/confirm-transaction';

if (!mockRpcEnabled) {
  // Testing max commitment level takes around 20s to complete
  jest.setTimeout(30000);
}

const expectedData = (authorizedPubkey: PublicKey): string => {
  const expectedData = Buffer.alloc(NONCE_ACCOUNT_LENGTH);
  expectedData.writeInt32LE(0, 0); // Version, 4 bytes
  expectedData.writeInt32LE(1, 4); // State, 4 bytes
  authorizedPubkey.toBuffer().copy(expectedData, 8); // authorizedPubkey, 32 bytes
  const mockNonce = new Account();
  mockNonce.publicKey.toBuffer().copy(expectedData, 40); // Hash, 32 bytes
  expectedData.writeUInt16LE(5000, 72); // feeCalculator, 8 bytes
  return expectedData.toString('base64');
};

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
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [from.publicKey.toBase58(), minimumAmount * 2],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const signature = await connection.requestAirdrop(
    from.publicKey,
    minimumAmount * 2,
  );
  mockConfirmTransaction(signature);
  await connection.confirmTransaction(signature, 0);

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

  mockGetRecentBlockhash('max');
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
  const nonceSignature = await connection.sendTransaction(
    transaction,
    [from, nonceAccount],
    {
      skipPreflight: true,
    },
  );
  mockConfirmTransaction(nonceSignature);
  await connection.confirmTransaction(nonceSignature, 0);

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
          data: expectedData(from.publicKey),
          executable: false,
        },
      },
    },
  ]);
  //
  const nonceAccountData = await connection.getNonce(nonceAccount.publicKey);
  if (nonceAccountData === null) {
    expect(nonceAccountData).not.toBeNull();
    return;
  }
  expect(nonceAccountData.authorizedPubkey).toEqual(from.publicKey);
  expect(bs58.decode(nonceAccountData.nonce).length).toBeGreaterThan(30);
});

test('create and query nonce account with seed', async () => {
  const from = new Account();
  const seed = 'seed';
  const noncePubkey = await PublicKey.createWithSeed(
    from.publicKey,
    seed,
    SystemProgram.programId,
  );
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
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [from.publicKey.toBase58(), minimumAmount * 2],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const signature = await connection.requestAirdrop(
    from.publicKey,
    minimumAmount * 2,
  );
  mockConfirmTransaction(signature);
  await connection.confirmTransaction(signature, 0);

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

  mockGetRecentBlockhash('max');
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
    noncePubkey: noncePubkey,
    basePubkey: from.publicKey,
    seed,
    authorizedPubkey: from.publicKey,
    lamports: minimumAmount,
  });
  const nonceSignature = await connection.sendTransaction(transaction, [from], {
    skipPreflight: true,
  });
  mockConfirmTransaction(nonceSignature);
  await connection.confirmTransaction(nonceSignature, 0);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [noncePubkey.toBase58(), {commitment: 'recent'}],
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
          data: expectedData(from.publicKey),
          executable: false,
        },
      },
    },
  ]);
  //
  const nonceAccountData = await connection.getNonce(noncePubkey);
  if (nonceAccountData === null) {
    expect(nonceAccountData).not.toBeNull();
    return;
  }
  expect(nonceAccountData.authorizedPubkey).toEqual(from.publicKey);
  expect(bs58.decode(nonceAccountData.nonce).length).toBeGreaterThan(30);
});
