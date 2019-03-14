// @flow
import {
  Account,
  Connection,
  BpfLoader,
  Loader,
  SystemProgram,
  sendAndConfirmTransaction,
} from '../src';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from '../src/timing';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';
import {sleep} from '../src/util/sleep';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(30000);
}

const errorMessage = 'Invalid request';
const errorResponse = {
  error: {
    message: errorMessage,
  },
  result: undefined,
};

test('get account info - error', () => {
  const account = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58()],
    },
    errorResponse,
  ]);

  expect(connection.getAccountInfo(account.publicKey)).rejects.toThrow(
    errorMessage,
  );
});

test('get balance', async () => {
  const account = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [account.publicKey.toBase58()],
    },
    {
      error: null,
      result: 0,
    },
  ]);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBeGreaterThanOrEqual(0);
});

test('confirm transaction - error', () => {
  const connection = new Connection(url);

  const badTransactionSignature = 'bad transaction signature';

  mockRpc.push([
    url,
    {
      method: 'confirmTransaction',
      params: [badTransactionSignature],
    },
    errorResponse,
  ]);

  expect(
    connection.confirmTransaction(badTransactionSignature),
  ).rejects.toThrow(errorMessage);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatus',
      params: [badTransactionSignature],
    },
    errorResponse,
  ]);

  expect(
    connection.getSignatureStatus(badTransactionSignature),
  ).rejects.toThrow(errorMessage);
});

test('get transaction count', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getTransactionCount',
      params: [],
    },
    {
      error: null,
      result: 1000000,
    },
  ]);

  const count = await connection.getTransactionCount();
  expect(count).toBeGreaterThanOrEqual(0);
});

test('get recent blockhash', async () => {
  const connection = new Connection(url);

  mockGetRecentBlockhash();

  const recentBlockhash = await connection.getRecentBlockhash();
  expect(recentBlockhash.length).toBeGreaterThanOrEqual(43);
});

test('request airdrop', async () => {
  const account = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account.publicKey.toBase58(), 40],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account.publicKey.toBase58(), 2],
    },
    {
      error: null,
      result:
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [account.publicKey.toBase58()],
    },
    {
      error: null,
      result: 42,
    },
  ]);

  await connection.requestAirdrop(account.publicKey, 40);
  await connection.requestAirdrop(account.publicKey, 2);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBe(42);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58()],
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
        lamports: 42,
        data: [],
        executable: false,
      },
    },
  ]);

  const accountInfo = await connection.getAccountInfo(account.publicKey);
  expect(accountInfo.lamports).toBe(42);
  expect(accountInfo.data).toHaveLength(0);
  expect(accountInfo.owner).toEqual(SystemProgram.programId);
});

test('transaction', async () => {
  const accountFrom = new Account();
  const accountTo = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [accountFrom.publicKey.toBase58(), 12],
    },
    {
      error: null,
      result:
        '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58()],
    },
    {
      error: null,
      result: 12,
    },
  ]);
  await connection.requestAirdrop(accountFrom.publicKey, 12);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(12);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [accountTo.publicKey.toBase58(), 21],
    },
    {
      error: null,
      result:
        '8WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountTo.publicKey.toBase58()],
    },
    {
      error: null,
      result: 21,
    },
  ]);
  await connection.requestAirdrop(accountTo.publicKey, 21);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(21);

  mockGetRecentBlockhash();
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

  const transaction = SystemProgram.move(
    accountFrom.publicKey,
    accountTo.publicKey,
    10,
  );
  transaction.fee = 0;
  const signature = await connection.sendTransaction(transaction, accountFrom);

  mockRpc.push([
    url,
    {
      method: 'confirmTransaction',
      params: [
        '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
      ],
    },
    {
      error: null,
      result: true,
    },
  ]);

  let i = 0;
  for (;;) {
    if (await connection.confirmTransaction(signature)) {
      break;
    }

    expect(mockRpcEnabled).toBe(false);
    expect(++i).toBeLessThan(10);
    await sleep(500);
  }

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatus',
      params: [
        '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
      ],
    },
    {
      error: null,
      result: 'Confirmed',
    },
  ]);
  await expect(connection.getSignatureStatus(signature)).resolves.toBe(
    'Confirmed',
  );

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58()],
    },
    {
      error: null,
      result: 2,
    },
  ]);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(2);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountTo.publicKey.toBase58()],
    },
    {
      error: null,
      result: 31,
    },
  ]);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(31);
});

test('multi-instruction transaction', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const accountFrom = new Account();
  const accountTo = new Account();
  const connection = new Connection(url);

  await connection.requestAirdrop(accountFrom.publicKey, 12);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(12);

  await connection.requestAirdrop(accountTo.publicKey, 21);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(21);

  // 1. Move(accountFrom, accountTo)
  // 2. Move(accountTo, accountFrom)
  const transaction = SystemProgram.move(
    accountFrom.publicKey,
    accountTo.publicKey,
    10,
  ).add(SystemProgram.move(accountTo.publicKey, accountFrom.publicKey, 10));
  transaction.fee = 0;
  const signature = await connection.sendTransaction(
    transaction,
    accountFrom,
    accountTo,
  );
  let i = 0;
  for (;;) {
    if (await connection.confirmTransaction(signature)) {
      break;
    }

    expect(mockRpcEnabled).toBe(false);
    expect(++i).toBeLessThan(10);
    await sleep(500);
  }
  await expect(connection.getSignatureStatus(signature)).resolves.toBe(
    'Confirmed',
  );

  expect(await connection.getBalance(accountFrom.publicKey)).toBe(12);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(21);
});

test('account change notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const owner = new Account();
  const programAccount = new Account();

  const mockCallback = jest.fn();

  const subscriptionId = connection.onAccountChange(
    programAccount.publicKey,
    mockCallback,
  );

  await connection.requestAirdrop(owner.publicKey, 42);
  const transaction = SystemProgram.createAccount(
    owner.publicKey,
    programAccount.publicKey,
    42,
    3,
    BpfLoader.programId,
  );
  transaction.fee = 0;
  await sendAndConfirmTransaction(connection, transaction, owner);

  const loader = new Loader(connection, BpfLoader.programId);
  await loader.load(programAccount, [1, 2, 3]);

  // Wait for mockCallback to receive a call
  let i = 0;
  for (;;) {
    if (mockCallback.mock.calls.length === 1) {
      break;
    }

    if (++i === 5) {
      console.log(JSON.stringify(mockCallback.mock.calls));
      throw new Error('mockCallback should be called twice');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  await connection.removeAccountChangeListener(subscriptionId);

  expect(mockCallback.mock.calls[0][0].lamports).toBe(41);
  expect(mockCallback.mock.calls[0][0].owner).toEqual(BpfLoader.programId);
  expect(mockCallback.mock.calls[0][0].executable).toBe(false);
  expect(mockCallback.mock.calls[0][0].data).toEqual(
    Buffer.from([1, 2, 3]),
  );
});

test('program account change notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const owner = new Account();
  const programAccount = new Account();

  const mockCallback = jest.fn();

  const subscriptionId = connection.onProgramAccountChange(
    BpfLoader.programId,
    mockCallback,
  );

  await connection.requestAirdrop(owner.publicKey, 42);
  const transaction = SystemProgram.createAccount(
    owner.publicKey,
    programAccount.publicKey,
    42,
    3,
    BpfLoader.programId,
  );
  transaction.fee = 0;
  await sendAndConfirmTransaction(connection, transaction, owner);

  const loader = new Loader(connection, BpfLoader.programId);
  await loader.load(programAccount, [1, 2, 3]);

  // Wait for mockCallback to receive a call
  let i = 0;
  for (;;) {
    if (mockCallback.mock.calls.length === 1) {
      break;
    }

    if (++i === 5) {
      console.log(JSON.stringify(mockCallback.mock.calls));
      throw new Error('mockCallback should be called twice');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  await connection.removeProgramAccountChangeListener(subscriptionId);

  expect(mockCallback.mock.calls[0][0].accountId).toEqual(
    programAccount.publicKey.toString(),
  );
  expect(mockCallback.mock.calls[0][0].accountInfo.lamports).toBe(41);
  expect(mockCallback.mock.calls[0][0].accountInfo.owner).toEqual(
    BpfLoader.programId,
  );
  expect(mockCallback.mock.calls[0][0].accountInfo.executable).toBe(false);
  expect(mockCallback.mock.calls[0][0].accountInfo.data).toEqual(
    Buffer.from([1, 2, 3]),
  );
});
