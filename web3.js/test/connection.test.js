// @flow
import bs58 from 'bs58';

import {
  Account,
  Connection,
  SystemProgram,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  PublicKey,
} from '../src';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from '../src/timing';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';
import {sleep} from '../src/util/sleep';
import {BLOCKHASH_CACHE_TIMEOUT_MS} from '../src/connection';
import type {SignatureStatus, TransactionError} from '../src/connection';
import {mockConfirmTransaction} from './mockrpc/confirm-transaction';

// Testing blockhash cache takes around 30s to complete
jest.setTimeout(40000);

const errorMessage = 'Invalid';
const errorResponse = {
  error: {
    code: -32602,
    message: errorMessage,
  },
  result: undefined,
};

const verifySignatureStatus = (
  status: SignatureStatus | null,
  err?: TransactionError,
): SignatureStatus => {
  if (status === null) {
    expect(status).not.toBeNull();
    throw new Error(); // unreachable
  }

  const expectedErr = err || null;
  expect(status.err).toEqual(expectedErr);
  expect(status.slot).toBeGreaterThanOrEqual(0);
  if (expectedErr !== null) return status;

  const confirmations = status.confirmations;
  if (typeof confirmations === 'number') {
    expect(confirmations).toBeGreaterThanOrEqual(0);
  } else {
    expect(confirmations).toBeNull();
  }
  return status;
};

test('get account info - not found', async () => {
  const account = new Account();
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58()],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: null,
      },
    },
  ]);

  expect(await connection.getAccountInfo(account.publicKey)).toBeNull();
});

test('get program accounts', async () => {
  const connection = new Connection(url, 'recent');
  const account0 = new Account();
  const account1 = new Account();
  const programId = new Account();
  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account0.publicKey.toBase58(), LAMPORTS_PER_SOL],
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
      method: 'requestAirdrop',
      params: [account1.publicKey.toBase58(), 0.5 * LAMPORTS_PER_SOL],
    },
    {
      error: null,
      result:
        '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  await connection.requestAirdrop(account0.publicKey, LAMPORTS_PER_SOL);
  await connection.requestAirdrop(account1.publicKey, 0.5 * LAMPORTS_PER_SOL);

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
  let transaction = SystemProgram.assign({
    accountPubkey: account0.publicKey,
    programId: programId.publicKey,
  });
  await sendAndConfirmTransaction(connection, transaction, [account0], {
    confirmations: 1,
    skipPreflight: true,
  });

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

  transaction = SystemProgram.assign({
    accountPubkey: account1.publicKey,
    programId: programId.publicKey,
  });

  await sendAndConfirmTransaction(connection, transaction, [account1], {
    confirmations: 1,
    skipPreflight: true,
  });

  mockRpc.push([
    url,
    {
      method: 'getFeeCalculatorForBlockhash',
      params: [transaction.recentBlockhash, {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: {
          feeCalculator: {
            lamportsPerSignature: 42,
          },
        },
      },
    },
  ]);

  if (transaction.recentBlockhash === null) {
    expect(transaction.recentBlockhash).not.toBeNull();
    return;
  }

  const feeCalculator = (
    await connection.getFeeCalculatorForBlockhash(transaction.recentBlockhash)
  ).value;

  if (feeCalculator === null) {
    expect(feeCalculator).not.toBeNull();
    return;
  }

  mockRpc.push([
    url,
    {
      method: 'getProgramAccounts',
      params: [programId.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: [
        {
          account: {
            data: '',
            executable: false,
            lamports: LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
            owner: programId.publicKey.toBase58(),
            rentEpoch: 20,
          },
          pubkey: account0.publicKey.toBase58(),
        },
        {
          account: {
            data: '',
            executable: false,
            lamports:
              0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
            owner: programId.publicKey.toBase58(),
            rentEpoch: 20,
          },
          pubkey: account1.publicKey.toBase58(),
        },
      ],
    },
  ]);
  const programAccounts = await connection.getProgramAccounts(
    programId.publicKey,
  );
  expect(programAccounts.length).toBe(2);

  programAccounts.forEach(function (element) {
    expect([
      account0.publicKey.toBase58(),
      account1.publicKey.toBase58(),
    ]).toEqual(expect.arrayContaining([element.pubkey]));
    if (element.pubkey == account0.publicKey) {
      expect(element.account.lamports).toBe(
        LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
      );
    } else {
      expect(element.account.lamports).toBe(
        0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
      );
    }
  });
});

test('validatorExit', async () => {
  if (!mockRpcEnabled) {
    console.log('validatorExit skipped on live node');
    return;
  }
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'validatorExit',
    },
    {
      error: null,
      result: false,
    },
  ]);

  const result = await connection.validatorExit();
  expect(result).toBe(false);
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
      result: {
        context: {
          slot: 11,
        },
        value: 0,
      },
    },
  ]);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBeGreaterThanOrEqual(0);
});

test('get inflation', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getInflationGovernor',
      params: [],
    },
    {
      error: null,
      result: {
        foundation: 0.05,
        foundationTerm: 7.0,
        initial: 0.15,
        taper: 0.15,
        terminal: 0.015,
      },
    },
  ]);

  const inflation = await connection.getInflationGovernor();

  for (const key of [
    'initial',
    'terminal',
    'taper',
    'foundation',
    'foundationTerm',
  ]) {
    expect(inflation).toHaveProperty(key);
    expect(inflation[key]).toBeGreaterThan(0);
  }
});

test('get epoch info', async () => {
  const connection = new Connection(url, 'recent');

  mockRpc.push([
    url,
    {
      method: 'getEpochInfo',
      params: [{commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        epoch: 0,
        slotIndex: 1,
        slotsInEpoch: 8192,
        absoluteSlot: 1,
        blockHeight: 1,
      },
    },
  ]);

  const epochInfo = await connection.getEpochInfo();

  for (const key of [
    'epoch',
    'slotIndex',
    'slotsInEpoch',
    'absoluteSlot' /*, 'blockHeight'*/, // Uncomment blockHeight after 1.1.20 ships
  ]) {
    expect(epochInfo).toHaveProperty(key);
    expect(epochInfo[key]).toBeGreaterThanOrEqual(0);
  }
});

test('get epoch schedule', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getEpochSchedule',
      params: [],
    },
    {
      error: null,
      result: {
        firstNormalEpoch: 8,
        firstNormalSlot: 8160,
        leaderScheduleSlotOffset: 8192,
        slotsPerEpoch: 8192,
        warmup: true,
      },
    },
  ]);

  const epochSchedule = await connection.getEpochSchedule();

  for (const key of [
    'firstNormalEpoch',
    'firstNormalSlot',
    'leaderScheduleSlotOffset',
    'slotsPerEpoch',
  ]) {
    expect(epochSchedule).toHaveProperty('warmup');
    expect(epochSchedule).toHaveProperty(key);
    if (epochSchedule.warmup) {
      expect(epochSchedule[key]).toBeGreaterThan(0);
    }
  }
});

test('get leader schedule', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getLeaderSchedule',
      params: [],
    },
    {
      error: null,
      result: {
        '123vij84ecQEKUvQ7gYMKxKwKF6PbYSzCzzURYA4xULY': [0, 1, 2, 3],
        '8PTjAikKoAybKXcEPnDSoy8wSNNikUBJ1iKawJKQwXnB': [4, 5, 6, 7],
      },
    },
  ]);

  const leaderSchedule = await connection.getLeaderSchedule();
  expect(Object.keys(leaderSchedule).length).toBeGreaterThanOrEqual(1);
  for (const key in leaderSchedule) {
    const slots = leaderSchedule[key];
    expect(Array.isArray(slots)).toBe(true);
    expect(slots.length).toBeGreaterThanOrEqual(4);
  }
});

test('get slot', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSlot',
    },
    {
      error: null,
      result: 123,
    },
  ]);

  const slotLeader = await connection.getSlot();
  if (mockRpcEnabled) {
    expect(slotLeader).toBe(123);
  } else {
    // No idea what the correct slot value should be on a live cluster, so
    // just check the type
    expect(typeof slotLeader).toBe('number');
  }
});

test('get slot leader', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSlotLeader',
    },
    {
      error: null,
      result: '11111111111111111111111111111111',
    },
  ]);

  const slotLeader = await connection.getSlotLeader();
  if (mockRpcEnabled) {
    expect(slotLeader).toBe('11111111111111111111111111111111');
  } else {
    // No idea what the correct slotLeader value should be on a live cluster, so
    // just check the type
    expect(typeof slotLeader).toBe('string');
  }
});

test('get cluster nodes', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getClusterNodes',
    },
    {
      error: null,
      result: [
        {
          pubkey: '11111111111111111111111111111111',
          gossip: '127.0.0.0:1234',
          tpu: '127.0.0.0:1235',
          rpc: null,
          version: '1.1.10',
        },
      ],
    },
  ]);

  const clusterNodes = await connection.getClusterNodes();
  if (mockRpcEnabled) {
    expect(clusterNodes).toHaveLength(1);
    expect(clusterNodes[0].pubkey).toBe('11111111111111111111111111111111');
    expect(typeof clusterNodes[0].gossip).toBe('string');
    expect(typeof clusterNodes[0].tpu).toBe('string');
    expect(clusterNodes[0].rpc).toBeNull();
  } else {
    // There should be at least one node (the node that we're talking to)
    expect(clusterNodes.length).toBeGreaterThan(0);
  }
});

test('getVoteAccounts', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url);
  const voteAccounts = await connection.getVoteAccounts();
  expect(
    voteAccounts.current.concat(voteAccounts.delinquent).length,
  ).toBeGreaterThan(0);
});

test('confirm transaction - error', async () => {
  const connection = new Connection(url);

  const badTransactionSignature = 'bad transaction signature';

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [[badTransactionSignature]],
    },
    errorResponse,
  ]);

  await expect(
    connection.confirmTransaction(badTransactionSignature),
  ).rejects.toThrow(errorMessage);

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [[badTransactionSignature]],
    },
    errorResponse,
  ]);

  await expect(
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

test('get total supply', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getTotalSupply',
      params: [],
    },
    {
      error: null,
      result: 1000000,
    },
  ]);

  const count = await connection.getTotalSupply();
  expect(count).toBeGreaterThanOrEqual(0);
});

test('get minimum balance for rent exemption', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [512],
    },
    {
      error: null,
      result: 1000000,
    },
  ]);

  const count = await connection.getMinimumBalanceForRentExemption(512);
  expect(count).toBeGreaterThanOrEqual(0);
});

test('get confirmed signatures for address', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSlot',
      params: [],
    },
    {
      error: null,
      result: 1,
    },
  ]);

  while ((await connection.getSlot()) <= 0) {
    continue;
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedBlock',
      params: [1],
    },
    {
      error: null,
      result: {
        blockhash: '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 0,
        transactions: [
          {
            meta: {
              fee: 10000,
              postBalances: [499260347380, 15298080, 1, 1, 1],
              preBalances: [499260357380, 15298080, 1, 1, 1],
              status: {Ok: null},
              err: null,
            },
            transaction: {
              message: {
                accountKeys: [
                  'va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf',
                  '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
                  'SysvarS1otHashes111111111111111111111111111',
                  'SysvarC1ock11111111111111111111111111111111',
                  'Vote111111111111111111111111111111111111111',
                ],
                header: {
                  numReadonlySignedAccounts: 0,
                  numReadonlyUnsignedAccounts: 3,
                  numRequiredSignatures: 2,
                },
                instructions: [
                  {
                    accounts: [1, 2, 3],
                    data:
                      '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                    programIdIndex: 4,
                  },
                ],
                recentBlockhash: 'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
              },
              signatures: [
                'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
                '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
              ],
            },
          },
        ],
      },
    },
  ]);

  // Find a block that has a transaction, usually Block 1
  let slot = 0;
  let address: ?PublicKey;
  let expectedSignature: ?string;
  while (!address || !expectedSignature) {
    slot++;
    const block = await connection.getConfirmedBlock(slot);
    if (block.transactions.length > 0) {
      const {
        signature,
        publicKey,
      } = block.transactions[0].transaction.signatures[0];
      if (signature) {
        address = publicKey;
        expectedSignature = bs58.encode(signature);
      }
    }
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedSignaturesForAddress',
      params: [address.toBase58(), slot, slot + 1],
    },
    {
      error: null,
      result: [expectedSignature],
    },
  ]);

  const confirmedSignatures = await connection.getConfirmedSignaturesForAddress(
    address,
    slot,
    slot + 1,
  );
  expect(confirmedSignatures.includes(expectedSignature)).toBe(true);

  const badSlot = Number.MAX_SAFE_INTEGER - 1;
  mockRpc.push([
    url,
    {
      method: 'getConfirmedSignaturesForAddress',
      params: [address.toBase58(), badSlot, badSlot + 1],
    },
    {
      error: null,
      result: [],
    },
  ]);

  const emptySignatures = await connection.getConfirmedSignaturesForAddress(
    address,
    badSlot,
    badSlot + 1,
  );
  expect(emptySignatures.length).toBe(0);
});

test('get confirmed transaction', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSlot',
      params: [],
    },
    {
      error: null,
      result: 1,
    },
  ]);

  while ((await connection.getSlot()) <= 0) {
    continue;
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedBlock',
      params: [1],
    },
    {
      error: null,
      result: {
        blockhash: '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 0,
        transactions: [
          {
            meta: {
              fee: 10000,
              postBalances: [499260347380, 15298080, 1, 1, 1],
              preBalances: [499260357380, 15298080, 1, 1, 1],
              status: {Ok: null},
              err: null,
            },
            transaction: {
              message: {
                accountKeys: [
                  'va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf',
                  '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
                  'SysvarS1otHashes111111111111111111111111111',
                  'SysvarC1ock11111111111111111111111111111111',
                  'Vote111111111111111111111111111111111111111',
                ],
                header: {
                  numReadonlySignedAccounts: 0,
                  numReadonlyUnsignedAccounts: 3,
                  numRequiredSignatures: 2,
                },
                instructions: [
                  {
                    accounts: [1, 2, 3],
                    data:
                      '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                    programIdIndex: 4,
                  },
                ],
                recentBlockhash: 'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
              },
              signatures: [
                'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
                '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
              ],
            },
          },
        ],
      },
    },
  ]);

  // Find a block that has a transaction, usually Block 1
  let slot = 0;
  let confirmedTransaction: ?string;
  while (!confirmedTransaction) {
    slot++;
    const block = await connection.getConfirmedBlock(slot);
    for (const tx of block.transactions) {
      if (tx.transaction.signature) {
        confirmedTransaction = bs58.encode(tx.transaction.signature);
      }
    }
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedTransaction',
      params: [confirmedTransaction],
    },
    {
      error: null,
      result: {
        slot,
        transaction: {
          message: {
            accountKeys: [
              'va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf',
              '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
              'SysvarS1otHashes111111111111111111111111111',
              'SysvarC1ock11111111111111111111111111111111',
              'Vote111111111111111111111111111111111111111',
            ],
            header: {
              numReadonlySignedAccounts: 0,
              numReadonlyUnsignedAccounts: 3,
              numRequiredSignatures: 2,
            },
            instructions: [
              {
                accounts: [1, 2, 3],
                data:
                  '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                programIdIndex: 4,
              },
            ],
            recentBlockhash: 'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
          },
          signatures: [
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
            '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
          ],
        },
        meta: {
          fee: 10000,
          postBalances: [499260347380, 15298080, 1, 1, 1],
          preBalances: [499260357380, 15298080, 1, 1, 1],
          status: {Ok: null},
          err: null,
        },
      },
    },
  ]);

  const result = await connection.getConfirmedTransaction(confirmedTransaction);

  if (!result) {
    expect(result).toBeDefined();
    expect(result).not.toBeNull();
    return;
  }

  if (result.transaction.signature === null) {
    expect(result.transaction.signature).not.toBeNull();
    return;
  }

  const resultSignature = bs58.encode(result.transaction.signature);
  expect(resultSignature).toEqual(confirmedTransaction);

  const newAddress = new Account().publicKey;
  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [newAddress.toBase58(), 1],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const recentSignature = await connection.requestAirdrop(newAddress, 1);
  mockRpc.push([
    url,
    {
      method: 'getConfirmedTransaction',
      params: [recentSignature],
    },
    {
      error: null,
      result: null,
    },
  ]);

  const nullResponse = await connection.getConfirmedTransaction(
    recentSignature,
  );
  expect(nullResponse).toBeNull();
});

test('get confirmed block', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSlot',
      params: [],
    },
    {
      error: null,
      result: 1,
    },
  ]);

  while ((await connection.getSlot()) <= 0) {
    continue;
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedBlock',
      params: [0],
    },
    {
      error: null,
      result: {
        blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 0,
        transactions: [],
      },
    },
  ]);

  // Block 0 never has any transactions in automation localnet
  const block0 = await connection.getConfirmedBlock(0);
  const blockhash0 = block0.blockhash;
  expect(block0.transactions.length).toBe(0);
  expect(blockhash0).not.toBeNull();
  expect(block0.previousBlockhash).not.toBeNull();
  expect(block0.parentSlot).toBe(0);

  mockRpc.push([
    url,
    {
      method: 'getConfirmedBlock',
      params: [1],
    },
    {
      error: null,
      result: {
        blockhash: '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 0,
        transactions: [
          {
            meta: {
              fee: 10000,
              postBalances: [499260347380, 15298080, 1, 1, 1],
              preBalances: [499260357380, 15298080, 1, 1, 1],
              status: {Ok: null},
              err: null,
            },
            transaction: {
              message: {
                accountKeys: [
                  'va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf',
                  '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
                  'SysvarS1otHashes111111111111111111111111111',
                  'SysvarC1ock11111111111111111111111111111111',
                  'Vote111111111111111111111111111111111111111',
                ],
                header: {
                  numReadonlySignedAccounts: 0,
                  numReadonlyUnsignedAccounts: 3,
                  numRequiredSignatures: 2,
                },
                instructions: [
                  {
                    accounts: [1, 2, 3],
                    data:
                      '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                    programIdIndex: 4,
                  },
                ],
                recentBlockhash: 'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
              },
              signatures: [
                'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
                '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
              ],
            },
          },
        ],
      },
    },
  ]);

  // Find a block that has a transaction, usually Block 1
  let x = 1;
  while (x < 10) {
    const block1 = await connection.getConfirmedBlock(x);
    if (block1.transactions.length >= 1) {
      expect(block1.previousBlockhash).toBe(blockhash0);
      expect(block1.blockhash).not.toBeNull();
      expect(block1.parentSlot).toBe(0);
      expect(block1.transactions[0].transaction).not.toBeNull();
      break;
    }
    x++;
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedBlock',
      params: [Number.MAX_SAFE_INTEGER],
    },
    {
      error: null,
      result: null,
    },
  ]);
  await expect(
    connection.getConfirmedBlock(Number.MAX_SAFE_INTEGER),
  ).rejects.toThrow();
});

test('get recent blockhash', async () => {
  const connection = new Connection(url);
  for (const commitment of [
    'max',
    'recent',
    'root',
    'single',
    'singleGossip',
  ]) {
    mockGetRecentBlockhash(commitment);

    const {blockhash, feeCalculator} = await connection.getRecentBlockhash(
      commitment,
    );
    expect(blockhash.length).toBeGreaterThanOrEqual(43);
    expect(feeCalculator.lamportsPerSignature).toBeGreaterThanOrEqual(0);
  }
});

test('get fee calculator', async () => {
  const connection = new Connection(url);

  mockGetRecentBlockhash('recent');
  const {blockhash} = await connection.getRecentBlockhash('recent');

  mockRpc.push([
    url,
    {
      method: 'getFeeCalculatorForBlockhash',
      params: [blockhash, {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: {
          feeCalculator: {
            lamportsPerSignature: 5000,
          },
        },
      },
    },
  ]);

  const feeCalculator = (
    await connection.getFeeCalculatorForBlockhash(blockhash, 'recent')
  ).value;
  if (feeCalculator === null) {
    expect(feeCalculator).not.toBeNull();
    return;
  }
  expect(feeCalculator.lamportsPerSignature).toEqual(5000);
});

test('get block time', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getBlockTime',
      params: [1],
    },
    {
      error: null,
      result: 10000,
    },
  ]);

  const blockTime = await connection.getBlockTime(1);
  if (blockTime === null) {
    expect(blockTime).not.toBeNull();
  } else {
    expect(blockTime).toBeGreaterThan(0);
  }
});

test('get minimum ledger slot', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'minimumLedgerSlot',
      params: [],
    },
    {
      error: null,
      result: 0,
    },
  ]);

  const minimumLedgerSlot = await connection.getMinimumLedgerSlot();
  expect(minimumLedgerSlot).toBeGreaterThanOrEqual(0);
});

test('get first available block', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getFirstAvailableBlock',
      params: [],
    },
    {
      error: null,
      result: 0,
    },
  ]);

  const firstAvailableBlock = await connection.getFirstAvailableBlock();
  expect(firstAvailableBlock).toBeGreaterThanOrEqual(0);
});

test('get supply', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getSupply',
      params: [],
    },
    {
      error: null,
      result: {
        context: {
          slot: 1,
        },
        value: {
          total: 1000,
          circulating: 100,
          nonCirculating: 900,
          nonCirculatingAccounts: [new Account().publicKey.toBase58()],
        },
      },
    },
  ]);

  const supply = (await connection.getSupply()).value;
  expect(supply.total).toBeGreaterThan(0);
  expect(supply.circulating).toBeGreaterThan(0);
  expect(supply.nonCirculating).toBeGreaterThan(0);
  expect(supply.nonCirculatingAccounts.length).toBeGreaterThan(0);
});

test('get largest accounts', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getLargestAccounts',
      params: [],
    },
    {
      error: null,
      result: {
        context: {
          slot: 1,
        },
        value: new Array(20).fill(0).map(() => ({
          address: new Account().publicKey.toBase58(),
          lamports: 1000,
        })),
      },
    },
  ]);

  const largestAccounts = (await connection.getLargestAccounts()).value;
  expect(largestAccounts.length).toEqual(20);
});

test('getVersion', async () => {
  const connection = new Connection(url);

  mockRpc.push([
    url,
    {
      method: 'getVersion',
      params: [],
    },
    {
      error: null,
      result: {'solana-core': '0.20.4'},
    },
  ]);

  const version = await connection.getVersion();
  expect(version['solana-core']).toBeTruthy();
});

test('request airdrop', async () => {
  const account = new Account();
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
      params: [account.publicKey.toBase58(), minimumAmount + 42],
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const signature = await connection.requestAirdrop(
    account.publicKey,
    minimumAmount + 42,
  );

  mockRpc.push([
    url,
    {
      method: 'getSignatureStatuses',
      params: [
        [
          '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
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
            confirmations: null,
            status: {Ok: null},
            err: null,
          },
        ],
      },
    },
  ]);

  await connection.confirmTransaction(signature, 0);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [account.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount + 42,
      },
    },
  ]);

  const balance = await connection.getBalance(account.publicKey);
  expect(balance).toBe(minimumAmount + 42);

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: {
          owner: '11111111111111111111111111111111',
          lamports: minimumAmount + 42,
          data: '',
          executable: false,
        },
      },
    },
  ]);

  const accountInfo = await connection.getAccountInfo(account.publicKey);
  if (accountInfo === null) {
    expect(accountInfo).not.toBeNull();
    return;
  }
  expect(accountInfo.lamports).toBe(minimumAmount + 42);
  expect(accountInfo.data).toHaveLength(0);
  expect(accountInfo.owner).toEqual(SystemProgram.programId);
});

test('transaction failure', async () => {
  const account = new Account();
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
      params: [account.publicKey.toBase58(), minimumAmount + 100010],
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
      params: [account.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount + 100010,
      },
    },
  ]);
  await connection.requestAirdrop(account.publicKey, minimumAmount + 100010);
  expect(await connection.getBalance(account.publicKey)).toBe(
    minimumAmount + 100010,
  );

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
            err: null,
          },
        ],
      },
    },
  ]);

  const newAccount = new Account();
  let transaction = SystemProgram.createAccount({
    fromPubkey: account.publicKey,
    newAccountPubkey: newAccount.publicKey,
    lamports: 1000,
    space: 0,
    programId: SystemProgram.programId,
  });
  await sendAndConfirmTransaction(
    connection,
    transaction,
    [account, newAccount],
    {confirmations: 1, skipPreflight: true},
  );

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

  // This should fail because the account is already created
  transaction = SystemProgram.createAccount({
    fromPubkey: account.publicKey,
    newAccountPubkey: newAccount.publicKey,
    lamports: 10,
    space: 0,
    programId: SystemProgram.programId,
  });
  const signature = await connection.sendTransaction(
    transaction,
    [account, newAccount],
    {skipPreflight: true},
  );

  const expectedErr = {InstructionError: [0, {Custom: 0}]};
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
            status: {Err: expectedErr},
            err: expectedErr,
          },
        ],
      },
    },
  ]);

  // Wait for one confirmation
  const confirmResult = (await connection.confirmTransaction(signature, 1))
    .value;
  verifySignatureStatus(confirmResult, expectedErr);

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
            status: {Err: expectedErr},
            err: expectedErr,
          },
        ],
      },
    },
  ]);

  const response = (await connection.getSignatureStatus(signature)).value;
  verifySignatureStatus(response, expectedErr);
});

test('transaction', async () => {
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
      params: [accountFrom.publicKey.toBase58(), minimumAmount + 100010],
    },
    {
      error: null,
      result:
        '0WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  const airdropFromSig = await connection.requestAirdrop(
    accountFrom.publicKey,
    minimumAmount + 100010,
  );
  mockConfirmTransaction(airdropFromSig);
  await connection.confirmTransaction(airdropFromSig, 0);

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
        value: minimumAmount + 100010,
      },
    },
  ]);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(
    minimumAmount + 100010,
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [accountTo.publicKey.toBase58(), minimumAmount],
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
      method: 'getSignatureStatuses',
      params: [
        [
          '8WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
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
            confirmations: 0,
            status: {Ok: null},
            err: null,
          },
        ],
      },
    },
  ]);
  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountTo.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount,
      },
    },
  ]);
  const airdropToSig = await connection.requestAirdrop(
    accountTo.publicKey,
    minimumAmount,
  );
  await connection.confirmTransaction(airdropToSig, 0);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(minimumAmount);

  mockGetRecentBlockhash('max');
  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
    },
    {
      error: null,
      result:
        '1WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  const transaction = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 10,
  });
  const signature = await connection.sendTransaction(
    transaction,
    [accountFrom],
    {skipPreflight: true},
  );

  mockConfirmTransaction(signature);
  let confirmResult = (await connection.confirmTransaction(signature, 0)).value;
  verifySignatureStatus(confirmResult);

  mockGetRecentBlockhash('max');
  mockRpc.push([
    url,
    {
      method: 'sendTransaction',
    },
    {
      error: null,
      result:
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);

  // Send again and ensure that new blockhash is used
  const lastFetch = Date.now();
  const transaction2 = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 10,
  });
  const signature2 = await connection.sendTransaction(
    transaction2,
    [accountFrom],
    {skipPreflight: true},
  );
  expect(signature).not.toEqual(signature2);
  expect(transaction.recentBlockhash).not.toEqual(transaction2.recentBlockhash);

  mockConfirmTransaction(signature2);
  await connection.confirmTransaction(signature2, 0);

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

  // Send new transaction and ensure that same blockhash is used
  const transaction3 = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 9,
  });
  const signature3 = await connection.sendTransaction(
    transaction3,
    [accountFrom],
    {
      skipPreflight: true,
    },
  );
  expect(transaction2.recentBlockhash).toEqual(transaction3.recentBlockhash);

  mockConfirmTransaction(signature3);
  await connection.confirmTransaction(signature3, 0);

  // Sleep until blockhash cache times out
  await sleep(
    Math.max(0, 1000 + BLOCKHASH_CACHE_TIMEOUT_MS - (Date.now() - lastFetch)),
  );

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

  const transaction4 = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 13,
  });

  const signature4 = await connection.sendTransaction(
    transaction4,
    [accountFrom],
    {
      skipPreflight: true,
    },
  );
  mockConfirmTransaction(signature4);
  await connection.confirmTransaction(signature4, 0);

  expect(transaction4.recentBlockhash).not.toEqual(
    transaction3.recentBlockhash,
  );

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

  // accountFrom may have less than 100000 due to transaction fees
  const balance = await connection.getBalance(accountFrom.publicKey);
  expect(balance).toBeGreaterThan(0);
  expect(balance).toBeLessThanOrEqual(minimumAmount + 100000);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountTo.publicKey.toBase58(), {commitment: 'recent'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: minimumAmount + 42,
      },
    },
  ]);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(
    minimumAmount + 42,
  );
});

test('multi-instruction transaction', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const accountFrom = new Account();
  const accountTo = new Account();
  const connection = new Connection(url, 'recent');

  let signature = await connection.requestAirdrop(
    accountFrom.publicKey,
    LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature, 0);
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(
    LAMPORTS_PER_SOL,
  );

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'recent',
  );

  signature = await connection.requestAirdrop(
    accountTo.publicKey,
    minimumAmount + 21,
  );
  await connection.confirmTransaction(signature, 0);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(
    minimumAmount + 21,
  );

  // 1. Move(accountFrom, accountTo)
  // 2. Move(accountTo, accountFrom)
  const transaction = SystemProgram.transfer({
    fromPubkey: accountFrom.publicKey,
    toPubkey: accountTo.publicKey,
    lamports: 100,
  }).add(
    SystemProgram.transfer({
      fromPubkey: accountTo.publicKey,
      toPubkey: accountFrom.publicKey,
      lamports: 100,
    }),
  );
  signature = await connection.sendTransaction(
    transaction,
    [accountFrom, accountTo],
    {skipPreflight: true},
  );

  await connection.confirmTransaction(signature, 1);

  const response = (await connection.getSignatureStatus(signature)).value;
  if (response !== null) {
    expect(typeof response.slot).toEqual('number');
    expect(response.err).toBeNull();
  } else {
    expect(response).not.toBeNull();
  }

  // accountFrom may have less than LAMPORTS_PER_SOL due to transaction fees
  expect(await connection.getBalance(accountFrom.publicKey)).toBeGreaterThan(0);
  expect(
    await connection.getBalance(accountFrom.publicKey),
  ).toBeLessThanOrEqual(LAMPORTS_PER_SOL);

  expect(await connection.getBalance(accountTo.publicKey)).toBe(
    minimumAmount + 21,
  );
});

test('account change notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');
  const owner = new Account();
  const programAccount = new Account();

  const mockCallback = jest.fn();

  const subscriptionId = connection.onAccountChange(
    programAccount.publicKey,
    mockCallback,
    'recent',
  );

  const balanceNeeded = Math.max(
    await connection.getMinimumBalanceForRentExemption(0),
    1,
  );

  await connection.requestAirdrop(owner.publicKey, LAMPORTS_PER_SOL);
  try {
    let transaction = SystemProgram.transfer({
      fromPubkey: owner.publicKey,
      toPubkey: programAccount.publicKey,
      lamports: balanceNeeded,
    });
    await sendAndConfirmTransaction(connection, transaction, [owner], {
      confirmations: 1,
      skipPreflight: true,
    });
  } catch (err) {
    await connection.removeAccountChangeListener(subscriptionId);
    throw err;
  }

  // Wait for mockCallback to receive a call
  let i = 0;
  for (;;) {
    if (mockCallback.mock.calls.length > 0) {
      break;
    }

    if (++i === 30) {
      throw new Error('Account change notification not observed');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  await connection.removeAccountChangeListener(subscriptionId);

  expect(mockCallback.mock.calls[0][0].lamports).toBe(balanceNeeded);
  expect(mockCallback.mock.calls[0][0].owner).toEqual(SystemProgram.programId);
});

test('program account change notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');
  const owner = new Account();
  const programAccount = new Account();

  // const mockCallback = jest.fn();

  const balanceNeeded = await connection.getMinimumBalanceForRentExemption(0);

  let notified = false;
  const subscriptionId = connection.onProgramAccountChange(
    SystemProgram.programId,
    keyedAccountInfo => {
      if (keyedAccountInfo.accountId !== programAccount.publicKey.toString()) {
        //console.log('Ignoring another account', keyedAccountInfo);
        return;
      }
      expect(keyedAccountInfo.accountInfo.lamports).toBe(balanceNeeded);
      expect(keyedAccountInfo.accountInfo.owner).toEqual(
        SystemProgram.programId,
      );
      notified = true;
    },
  );

  await connection.requestAirdrop(owner.publicKey, LAMPORTS_PER_SOL);
  try {
    let transaction = SystemProgram.transfer({
      fromPubkey: owner.publicKey,
      toPubkey: programAccount.publicKey,
      lamports: balanceNeeded,
    });
    await sendAndConfirmTransaction(connection, transaction, [owner], {
      confirmations: 1,
      skipPreflight: true,
    });
  } catch (err) {
    await connection.removeProgramAccountChangeListener(subscriptionId);
    throw err;
  }

  // Wait for mockCallback to receive a call
  let i = 0;
  while (!notified) {
    //for (;;) {
    if (++i === 30) {
      throw new Error('Program change notification not observed');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  await connection.removeProgramAccountChangeListener(subscriptionId);
});

test('slot notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');

  let notified = false;
  const subscriptionId = connection.onSlotChange(slotInfo => {
    expect(slotInfo.parent).toBeDefined();
    expect(slotInfo.slot).toBeDefined();
    expect(slotInfo.root).toBeDefined();
    expect(slotInfo.slot).toBeGreaterThan(slotInfo.parent);
    expect(slotInfo.slot).toBeGreaterThanOrEqual(slotInfo.root);
    notified = true;
  });

  // Wait for mockCallback to receive a call
  let i = 0;
  while (!notified) {
    if (++i === 30) {
      throw new Error('Slot change notification not observed');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  await connection.removeSlotChangeListener(subscriptionId);
});

test('root notification', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'recent');

  let roots = [];
  const subscriptionId = connection.onRootChange(root => {
    roots.push(root);
  });

  // Wait for mockCallback to receive a call
  let i = 0;
  while (roots.length < 2) {
    if (++i === 30) {
      throw new Error('Root change notification not observed');
    }
    // Sleep for a 1/4 of a slot, notifications only occur after a block is
    // processed
    await sleep((250 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);
  }

  expect(roots[1]).toBeGreaterThan(roots[0]);
  await connection.removeRootChangeListener(subscriptionId);
});
