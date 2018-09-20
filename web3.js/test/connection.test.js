// @flow

import {Account} from '../src/account';
import {Connection} from '../src/connection';
import {SystemProgram} from '../src/system-program';
import {mockRpc} from './__mocks__/node-fetch';

const url = 'http://testnet.solana.com:8899';
//const url = 'http://localhost:8899';

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
      params: [account.publicKey],
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
      params: [account.publicKey],
    },
    {
      error: null,
      result: 0,
    }
  ]);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBeGreaterThanOrEqual(0);
});

test('get balance - error', () => {
  const connection = new Connection(url);

  const invalidPublicKey = 'not a public key';
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [invalidPublicKey],
    },
    errorResponse,
  ]);

  expect(connection.getBalance(invalidPublicKey))
  .rejects.toThrow(errorMessage);
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
      params: [account.publicKey, 40],
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
      params: [account.publicKey, 2],
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
      params: [account.publicKey],
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
      params: [account.publicKey],
    },
    {
      error: null,
      result: {
        contract_id: [
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
  expect(accountInfo.userdata).toBe(null);
  expect(accountInfo.programId).toBe(SystemProgram.programId);
});

test('request airdrop - error', () => {
  const invalidPublicKey = 'not a public key';
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [invalidPublicKey, 1],
    },
    errorResponse
  ]);

  expect(connection.requestAirdrop(invalidPublicKey, 1))
  .rejects.toThrow(errorMessage);
});

test('transaction', async () => {
  const accountFrom = new Account();
  const accountTo = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [accountFrom.publicKey, 12],
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
      params: [accountFrom.publicKey],
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
      params: [accountTo.publicKey, 21],
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
      params: [accountTo.publicKey],
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
      method: 'getBalance',
      params: [accountFrom.publicKey],
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
      params: [accountTo.publicKey],
    },
    {
      error: null,
      result: 31,
    }
  ]);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(31);
});
