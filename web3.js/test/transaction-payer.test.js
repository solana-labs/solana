// @flow
import {Account, Connection, SystemProgram, LAMPORTS_PER_SOL} from '../src';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(30000);
}

test('transaction-payer', async () => {
  const accountPayer = new Account();
  const accountFrom = new Account();
  const accountTo = new Account();
  const connection = new Connection(url, 'recent');

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [0, {commitment: 'recent'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'recent',
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [
        accountPayer.publicKey.toBase58(),
        LAMPORTS_PER_SOL,
        {commitment: 'recent'},
      ],
    },
    {
      error: null,
      result:
        '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  await connection.requestAirdrop(accountPayer.publicKey, LAMPORTS_PER_SOL);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [
        accountFrom.publicKey.toBase58(),
        minimumAmount + 12,
        {commitment: 'recent'},
      ],
    },
    {
      error: null,
      result:
        '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  await connection.requestAirdrop(accountFrom.publicKey, minimumAmount + 12);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [
        accountTo.publicKey.toBase58(),
        minimumAmount + 21,
        {commitment: 'recent'},
      ],
    },
    {
      error: null,
      result:
        '8WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  await connection.requestAirdrop(accountTo.publicKey, minimumAmount + 21);

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

  const transaction = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 10,
  });

  const signature = await connection.sendTransaction(transaction, [
    accountPayer,
    accountFrom,
  ]);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [
        [
          '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
        ],
      ],
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
            confirmations: 1,
            status: {Ok: null},
            err: null,
          },
        ],
      },
    },
  ]);

  await connection.confirmTransaction(signature, 1);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [
        [
          '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
        ],
      ],
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
            confirmations: 11,
            status: {Ok: null},
            err: null,
          },
        ],
      },
    },
  ]);
  const {value} = await connection.getSignatureStatus(signature);
  if (value !== null) {
    expect(typeof value.slot).toEqual('number');
    expect(value.err).toBeNull();
  } else {
    expect(value).not.toBeNull();
  }

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountPayer.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: LAMPORTS_PER_SOL - 1,
      },
    },
  ]);

  // accountPayer should be less than LAMPORTS_PER_SOL as it paid for the transaction
  // (exact amount less depends on the current cluster fees)
  const balance = await connection.getBalance(accountPayer.publicKey);
  expect(balance).toBeGreaterThan(0);
  expect(balance).toBeLessThanOrEqual(LAMPORTS_PER_SOL);

  // accountFrom should have exactly 2, since it didn't pay for the transaction
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount + 2,
      },
    },
  ]);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(
    minimumAmount + 2,
  );
});
