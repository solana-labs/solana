// @flow

import {
  Account,
  Connection,
  SystemProgram,
} from '../src';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';

if (!mockRpcEnabled) {
  // The default of 5 seconds is too slow for live testing sometimes
  jest.setTimeout(10000);
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

  expect(connection.getAccountInfo(account.publicKey))
  .rejects.toThrow(errorMessage);
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
    }
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
  ]
  );

  expect(connection.confirmTransaction(badTransactionSignature))
  .rejects.toThrow(errorMessage);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatus',
      params: [badTransactionSignature],
    },
    errorResponse,
  ]
  );

  expect(connection.getSignatureStatus(badTransactionSignature))
  .rejects.toThrow(errorMessage);
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
    }
  ]
  );

  const count = await connection.getTransactionCount();
  expect(count).toBeGreaterThanOrEqual(0);
});

test('get last Id', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getLastId',
      params: [],
    },
    {
      error: null,
      result: '2BjEqiiT43J6XskiHdz7aoocjPeWkCPiKD72SiFQsrA2',
    }
  ]
  );

  const lastId = await connection.getLastId();
  expect(lastId.length).toBeGreaterThanOrEqual(43);
});

test('get finality', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getFinality',
      params: [],
    },
    {
      error: null,
      result: 123,
    }
  ]
  );

  const finality = await connection.getFinality();
  expect(finality).toBeGreaterThanOrEqual(0);
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
      result: '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
  ]);
  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account.publicKey.toBase58(), 2],
    },
    {
      error: null,
      result: '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
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
    }
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
        program_id: [
          0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
          0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ],
        tokens: 42,
        userdata: [],
      }
    }
  ]);

  const accountInfo = await connection.getAccountInfo(account.publicKey);
  expect(accountInfo.tokens).toBe(42);
  expect(accountInfo.userdata).toHaveLength(0);
  expect(accountInfo.programId).toEqual(SystemProgram.programId);
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
      result: '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
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
    }
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
      result: '8WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
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
    }
  ]);
  await connection.requestAirdrop(accountTo.publicKey, 21);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(21);

  mockRpc.push([
    url,
    {
      method: 'getLastId',
      params: [],
    },
    {
      error: null,
      result: '2BjEqiiT43J6XskiHdz7aoocjPeWkCPiKD72SiFQsrA2',
    }
  ]
  );
  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
    },
    {
      error: null,
      result: '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    }
  ]
  );

  const transaction = SystemProgram.move(
    accountFrom.publicKey,
    accountTo.publicKey,
    10
  );
  const signature = await connection.sendTransaction(accountFrom, transaction);

  mockRpc.push([
    url,
    {
      method: 'confirmTransaction',
      params: [
        '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk'
      ],
    },
    {
      error: null,
      result: true,
    }
  ]
  );
  expect(connection.confirmTransaction(signature)).resolves.toBe(true);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatus',
      params: [
        '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk'
      ],
    },
    {
      error: null,
      result: 'Confirmed',
    }
  ]
  );
  expect(connection.getSignatureStatus(signature)).resolves.toBe('Confirmed');

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58()],
    },
    {
      error: null,
      result: 2,
    }
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
    }
  ]);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(31);
});
