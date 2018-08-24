// @flow

import {Connection} from '../src/connection';
import {Account} from '../src/account';
import {mockRpc} from './__mocks__/node-fetch';

const url = 'http://master.testnet.solana.com:8899';

// Define DOITLIVE in the environment to test against the real full node
// identified by `url` instead of using the mock
if (process.env.DOITLIVE) {
  console.log(`Note: node-fetch mock is disabled, testing live against ${url}`);
} else {
  jest.mock('node-fetch');
}

const errorMessage = 'Invalid request';
const errorResponse = {
  error: {
    message: errorMessage,
  },
  result: undefined,
};

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
      result: '1111111111111111111111111111111111111111111111',
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
      result: true,
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
      result: true,
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

test('send transaction - error', () => {
  const secretKey = Buffer.from([
    153, 218, 149, 89, 225, 94, 145, 62, 233, 171, 46, 83, 227,
    223, 173, 87, 93, 163, 59, 73, 190, 17, 37, 187, 146, 46, 51,
    73, 79, 73, 136, 40, 27, 47, 73, 9, 110, 62, 93, 189, 15, 207,
    169, 192, 192, 205, 146, 217, 171, 59, 33, 84, 75, 52, 213, 221,
    74, 101, 217, 139, 135, 139, 153, 34
  ]);
  const account = new Account(secretKey);
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
      params: [[
        78, 52, 48, 146, 162, 213, 83, 169, 128, 10, 82, 26, 145, 238,
        1, 130, 16, 44, 249, 99, 121, 55, 217, 72, 77, 41, 73, 227, 5,
        15, 125, 212, 186, 157, 182, 100, 232, 232, 39, 84, 5, 121, 172,
        137, 177, 248, 188, 224, 196, 102, 204, 43, 128, 243, 170, 157,
        134, 216, 209, 8, 211, 209, 44, 1
      ]],
    },
    errorResponse,
  ]);


  expect(connection.sendTokens(account, account.publicKey, 123))
  .rejects.toThrow(errorMessage);
});


