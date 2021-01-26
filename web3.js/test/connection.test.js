// @flow
import bs58 from 'bs58';
import {Token, u64} from '@solana/spl-token';

import {
  Account,
  Authorized,
  Connection,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  Lockup,
  PublicKey,
  StakeProgram,
} from '../src';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from '../src/timing';
import {mockRpc, mockRpcEnabled} from './__mocks__/node-fetch';
import {mockGetRecentBlockhash} from './mockrpc/get-recent-blockhash';
import {url} from './url';
import {sleep} from '../src/util/sleep';
import {BLOCKHASH_CACHE_TIMEOUT_MS} from '../src/connection';
import type {TransactionSignature} from '../src/transaction';
import type {SignatureStatus, TransactionError} from '../src/connection';
import {mockConfirmTransaction} from './mockrpc/confirm-transaction';
import {mockRpcSocket} from './__mocks__/rpc-websockets';

// Testing tokens and blockhash cache each take around 30s to complete
jest.setTimeout(90000);

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
      params: [account.publicKey.toBase58(), {encoding: 'base64'}],
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

  if (!mockRpcEnabled) {
    expect(
      (await connection.getParsedAccountInfo(account.publicKey)).value,
    ).toBeNull();
  }
});

test('get program accounts', async () => {
  const connection = new Connection(url, 'singleGossip');
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
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockConfirmTransaction(
    '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
  );
  let signature = await connection.requestAirdrop(
    account0.publicKey,
    LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature);

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account1.publicKey.toBase58(), 0.5 * LAMPORTS_PER_SOL],
    },
    {
      error: null,
      result:
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  mockConfirmTransaction(
    '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
  );
  signature = await connection.requestAirdrop(
    account1.publicKey,
    0.5 * LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature);

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

  let transaction = new Transaction().add(
    SystemProgram.assign({
      accountPubkey: account0.publicKey,
      programId: programId.publicKey,
    }),
  );

  mockConfirmTransaction(
    '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
  );
  await sendAndConfirmTransaction(connection, transaction, [account0], {
    commitment: 'singleGossip',
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

  transaction = new Transaction().add(
    SystemProgram.assign({
      accountPubkey: account1.publicKey,
      programId: programId.publicKey,
    }),
  );

  mockConfirmTransaction(
    '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
  );
  await sendAndConfirmTransaction(connection, transaction, [account1], {
    commitment: 'singleGossip',
  });

  mockRpc.push([
    url,
    {
      method: 'getFeeCalculatorForBlockhash',
      params: [transaction.recentBlockhash, {commitment: 'singleGossip'}],
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

  if (!transaction.recentBlockhash) {
    expect(transaction.recentBlockhash).toBeTruthy();
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
      params: [
        programId.publicKey.toBase58(),
        {commitment: 'singleGossip', encoding: 'base64'},
      ],
    },
    {
      error: null,
      result: [
        {
          account: {
            data: ['', 'base64'],
            executable: false,
            lamports: LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
            owner: programId.publicKey.toBase58(),
            rentEpoch: 20,
          },
          pubkey: account0.publicKey.toBase58(),
        },
        {
          account: {
            data: ['', 'base64'],
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
    if (element.pubkey.equals(account0.publicKey)) {
      expect(element.account.lamports).toBe(
        LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
      );
    } else if (element.pubkey.equals(account1.publicKey)) {
      expect(element.account.lamports).toBe(
        0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
      );
    } else {
      expect(element.pubkey.equals(account1.publicKey)).toBe(true);
    }
  });

  if (!mockRpcEnabled) {
    const programAccounts = await connection.getParsedProgramAccounts(
      programId.publicKey,
    );
    expect(programAccounts.length).toBe(2);

    programAccounts.forEach(function (element) {
      if (element.pubkey.equals(account0.publicKey)) {
        expect(element.account.lamports).toBe(
          LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
        );
      } else if (element.pubkey.equals(account1.publicKey)) {
        expect(element.account.lamports).toBe(
          0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
        );
      } else {
        expect(element.pubkey.equals(account1.publicKey)).toBe(true);
      }
    });
  }
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
  const connection = new Connection(url, 'singleGossip');

  mockRpc.push([
    url,
    {
      method: 'getEpochInfo',
      params: [{commitment: 'singleGossip'}],
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
    'absoluteSlot',
    'blockHeight',
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

  await expect(
    connection.confirmTransaction(badTransactionSignature),
  ).rejects.toThrow('signature must be base58 encoded');

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

  // getConfirmedSignaturesForAddress tests...
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

  // getConfirmedSignaturesForAddress2 tests...
  mockRpc.push([
    url,
    {
      method: 'getConfirmedSignaturesForAddress2',
      params: [address.toBase58(), {limit: 1}],
    },
    {
      error: null,
      result: [
        {
          signature: expectedSignature,
          slot,
          err: null,
          memo: null,
        },
      ],
    },
  ]);

  const confirmedSignatures2 = await connection.getConfirmedSignaturesForAddress2(
    address,
    {limit: 1},
  );
  expect(confirmedSignatures2.length).toBe(1);
  if (mockRpcEnabled) {
    expect(confirmedSignatures2[0].signature).toBe(expectedSignature);
    expect(confirmedSignatures2[0].slot).toBe(slot);
    expect(confirmedSignatures2[0].err).toBeNull();
    expect(confirmedSignatures2[0].memo).toBeNull();
  }
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

test('get parsed confirmed transaction coerces public keys of inner instructions', async () => {
  if (!mockRpcEnabled) {
    return;
  }

  const connection = new Connection(url);

  const confirmedTransaction: TransactionSignature =
    '4ADvAUQYxkh4qWKYE9QLW8gCLomGG94QchDLG4quvpBz1WqARYvzWQDDitKduAKspuy1DjcbnaDAnCAfnKpJYs48';

  function getMockData(inner) {
    return {
      error: null,
      result: {
        slot: 353050305,
        transaction: {
          message: {
            accountKeys: [
              {
                pubkey: 'va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf',
                signer: true,
                writable: true,
              },
            ],
            instructions: [
              {
                accounts: ['va12u4o9DipLEB2z4fuoHszroq1U9NcAB9aooFDPJSf'],
                data:
                  '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                programId: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
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
          innerInstructions: [
            {
              index: 0,
              instructions: [inner],
            },
          ],
          status: {Ok: null},
          err: null,
        },
      },
    };
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedTransaction',
      params: [confirmedTransaction, 'jsonParsed'],
    },
    getMockData({
      parsed: {},
      program: 'spl-token',
      programId: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
    }),
  ]);

  const result = await connection.getParsedConfirmedTransaction(
    confirmedTransaction,
  );

  if (
    result !== null &&
    result.meta &&
    result.meta.innerInstructions !== undefined &&
    result.meta.innerInstructions.length > 0
  ) {
    expect(
      result.meta.innerInstructions[0].instructions[0].programId,
    ).toBeInstanceOf(PublicKey);
  }

  mockRpc.push([
    url,
    {
      method: 'getConfirmedTransaction',
      params: [confirmedTransaction, 'jsonParsed'],
    },
    getMockData({
      accounts: [
        'EeJqWk5pczNjsqqY3jia9xfFNG1dD68te4s8gsdCuEk7',
        '6tVrjJhFm5SAvvdh6tysjotQurCSELpxuW3JaAAYeC1m',
      ],
      data: 'ai3535',
      programId: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
    }),
  ]);

  //$FlowFixMe
  const result2 = await connection.getParsedConfirmedTransaction(
    confirmedTransaction,
  );

  let instruction = result2.meta.innerInstructions[0].instructions[0];
  expect(instruction.programId).toBeInstanceOf(PublicKey);
  expect(instruction.accounts[0]).toBeInstanceOf(PublicKey);
  expect(instruction.accounts[1]).toBeInstanceOf(PublicKey);
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
      error: {
        message: `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
      },
      result: null,
    },
  ]);
  await expect(
    connection.getConfirmedBlock(Number.MAX_SAFE_INTEGER),
  ).rejects.toThrow(`Block not available for slot ${Number.MAX_SAFE_INTEGER}`);
});

test('get recent blockhash', async () => {
  const connection = new Connection(url);
  for (const commitment of [
    'max',
    'singleGossip',
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

  mockGetRecentBlockhash('singleGossip');
  const {blockhash} = await connection.getRecentBlockhash('singleGossip');

  mockRpc.push([
    url,
    {
      method: 'getFeeCalculatorForBlockhash',
      params: [blockhash, {commitment: 'singleGossip'}],
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
    await connection.getFeeCalculatorForBlockhash(blockhash, 'singleGossip')
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
    // TODO: enable after https://github.com/solana-labs/solana/issues/11849 fixed
    // expect(blockTime).not.toBeNull();
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
  expect(supply.nonCirculating).toBeGreaterThanOrEqual(0);
  expect(supply.nonCirculatingAccounts.length).toBeGreaterThanOrEqual(0);
});

test('get performance samples', async () => {
  const connection = new Connection(url);

  if (mockRpcEnabled) {
    mockRpc.push([
      url,
      {
        method: 'getRecentPerformanceSamples',
        params: [],
      },
      {
        error: null,
        result: [
          {
            slot: 1234,
            numTransactions: 1000,
            numSlots: 60,
            samplePeriodSecs: 60,
          },
        ],
      },
    ]);
  }

  const perfSamples = await connection.getRecentPerformanceSamples();
  expect(Array.isArray(perfSamples)).toBe(true);

  if (perfSamples.length > 0) {
    expect(perfSamples[0].slot).toBeGreaterThan(0);
    expect(perfSamples[0].numTransactions).toBeGreaterThan(0);
    expect(perfSamples[0].numSlots).toBeGreaterThan(0);
    expect(perfSamples[0].samplePeriodSecs).toBeGreaterThan(0);
  }
});

test('get performance samples limit too high', async () => {
  const connection = new Connection(url);

  if (mockRpcEnabled) {
    mockRpc.push([
      url,
      {
        method: 'getRecentPerformanceSamples',
        params: [100000],
      },
      {
        error: {
          code: -32602,
          message: 'Invalid limit; max 720',
        },
        result: null,
      },
    ]);
  }

  await expect(
    connection.getRecentPerformanceSamples(100000),
  ).rejects.toThrow();
});

const TOKEN_PROGRAM_ID = new PublicKey(
  'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
);

describe('token methods', () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'singleGossip');
  const newAccount = new Account().publicKey;

  let testToken: Token;
  let testTokenAccount: PublicKey;
  let testSignature: TransactionSignature;
  let testOwner: Account;

  // Setup token mints and accounts for token tests
  beforeAll(async () => {
    const payerAccount = new Account();
    await connection.confirmTransaction(
      await connection.requestAirdrop(payerAccount.publicKey, 100000000000),
    );

    const mintOwner = new Account();
    const accountOwner = new Account();
    const token = await Token.createMint(
      connection,
      payerAccount,
      mintOwner.publicKey,
      null,
      2,
      TOKEN_PROGRAM_ID,
    );

    const tokenAccount = await token.createAccount(accountOwner.publicKey);
    await token.mintTo(tokenAccount, mintOwner, [], 11111);

    const token2 = await Token.createMint(
      connection,
      payerAccount,
      mintOwner.publicKey,
      null,
      2,
      TOKEN_PROGRAM_ID,
    );

    const token2Account = await token2.createAccount(accountOwner.publicKey);
    await token2.mintTo(token2Account, mintOwner, [], 100);

    const tokenAccountDest = await token.createAccount(accountOwner.publicKey);
    testSignature = await token.transfer(
      tokenAccount,
      tokenAccountDest,
      accountOwner,
      [],
      new u64(1),
    );

    await connection.confirmTransaction(testSignature, 'max');

    testOwner = accountOwner;
    testToken = token;
    testTokenAccount = tokenAccount;
  });

  test('get token supply', async () => {
    const supply = (await connection.getTokenSupply(testToken.publicKey)).value;
    expect(supply.uiAmount).toEqual(111.11);
    expect(supply.decimals).toEqual(2);
    expect(supply.amount).toEqual('11111');

    await expect(connection.getTokenSupply(newAccount)).rejects.toThrow();
  });

  test('get token largest accounts', async () => {
    const largestAccounts = (
      await connection.getTokenLargestAccounts(testToken.publicKey)
    ).value;

    expect(largestAccounts.length).toEqual(2);
    const largestAccount = largestAccounts[0];
    expect(largestAccount.address.equals(testTokenAccount)).toBe(true);
    expect(largestAccount.amount).toEqual('11110');
    expect(largestAccount.decimals).toEqual(2);
    expect(largestAccount.uiAmount).toEqual(111.1);

    await expect(
      connection.getTokenLargestAccounts(newAccount),
    ).rejects.toThrow();
  });

  test('get confirmed token transaction', async () => {
    const parsedTx = await connection.getParsedConfirmedTransaction(
      testSignature,
    );
    if (parsedTx === null) {
      expect(parsedTx).not.toBeNull();
      return;
    }
    const {signatures, message} = parsedTx.transaction;
    expect(signatures[0]).toEqual(testSignature);
    const ix = message.instructions[0];
    if (ix.parsed) {
      expect(ix.program).toEqual('spl-token');
      expect(ix.programId.equals(TOKEN_PROGRAM_ID)).toBe(true);
    } else {
      expect('parsed' in ix).toBe(true);
    }

    const missingSignature =
      '45pGoC4Rr3fJ1TKrsiRkhHRbdUeX7633XAGVec6XzVdpRbzQgHhe6ZC6Uq164MPWtiqMg7wCkC6Wy3jy2BqsDEKf';
    const nullResponse = await connection.getParsedConfirmedTransaction(
      missingSignature,
    );

    expect(nullResponse).toBeNull();
  });

  test('get token account balance', async () => {
    const balance = (await connection.getTokenAccountBalance(testTokenAccount))
      .value;
    expect(balance.amount).toEqual('11110');
    expect(balance.decimals).toEqual(2);
    expect(balance.uiAmount).toEqual(111.1);

    await expect(
      connection.getTokenAccountBalance(newAccount),
    ).rejects.toThrow();
  });

  test('get parsed token account info', async () => {
    const accountInfo = (
      await connection.getParsedAccountInfo(testTokenAccount)
    ).value;
    if (accountInfo) {
      const data = accountInfo.data;
      if (data instanceof Buffer) {
        expect(data instanceof Buffer).toBe(false);
      } else {
        expect(data.program).toEqual('spl-token');
        expect(data.parsed).toBeTruthy();
      }
    }
  });

  test('get parsed token program accounts', async () => {
    const tokenAccounts = await connection.getParsedProgramAccounts(
      TOKEN_PROGRAM_ID,
    );
    tokenAccounts.forEach(({account}) => {
      expect(account.owner.equals(TOKEN_PROGRAM_ID)).toBe(true);
      const data = account.data;
      if (data instanceof Buffer) {
        expect(data instanceof Buffer).toBe(false);
      } else {
        expect(data.parsed).toBeTruthy();
        expect(data.program).toEqual('spl-token');
      }
    });
  });

  test('get parsed token accounts by owner', async () => {
    const tokenAccounts = (
      await connection.getParsedTokenAccountsByOwner(testOwner.publicKey, {
        mint: testToken.publicKey,
      })
    ).value;
    tokenAccounts.forEach(({account}) => {
      expect(account.owner.equals(TOKEN_PROGRAM_ID)).toBe(true);
      const data = account.data;
      if (data instanceof Buffer) {
        expect(data instanceof Buffer).toBe(false);
      } else {
        expect(data.parsed).toBeTruthy();
        expect(data.program).toEqual('spl-token');
      }
    });
  });

  test('get token accounts by owner', async () => {
    const accountsWithMintFilter = (
      await connection.getTokenAccountsByOwner(testOwner.publicKey, {
        mint: testToken.publicKey,
      })
    ).value;
    expect(accountsWithMintFilter.length).toEqual(2);

    const accountsWithProgramFilter = (
      await connection.getTokenAccountsByOwner(testOwner.publicKey, {
        programId: TOKEN_PROGRAM_ID,
      })
    ).value;
    expect(accountsWithProgramFilter.length).toEqual(3);

    const noAccounts = (
      await connection.getTokenAccountsByOwner(newAccount, {
        mint: testToken.publicKey,
      })
    ).value;
    expect(noAccounts.length).toEqual(0);

    await expect(
      connection.getTokenAccountsByOwner(testOwner.publicKey, {
        mint: newAccount,
      }),
    ).rejects.toThrow();

    await expect(
      connection.getTokenAccountsByOwner(testOwner.publicKey, {
        programId: newAccount,
      }),
    ).rejects.toThrow();
  });
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

test('stake activation should throw when called for not delegated account', async () => {
  const connection = new Connection(url);

  const publicKey = new Account().publicKey;
  mockRpc.push([
    url,
    {
      method: 'getStakeActivation',
      params: [publicKey.toBase58(), {}],
    },
    {
      error: {message: 'account not delegated'},
      result: undefined,
    },
  ]);

  await expect(connection.getStakeActivation(publicKey)).rejects.toThrow();
});

test('stake activation should return activating for new accounts', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection(url, 'singleGossip');
  const voteAccounts = await connection.getVoteAccounts();
  const voteAccount = voteAccounts.current.concat(voteAccounts.delinquent)[0];
  const votePubkey = new PublicKey(voteAccount.votePubkey);

  const authorized = new Account();
  let signature = await connection.requestAirdrop(
    authorized.publicKey,
    2 * LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    StakeProgram.space,
  );

  const newStakeAccount = new Account();
  let createAndInitialize = StakeProgram.createAccount({
    fromPubkey: authorized.publicKey,
    stakePubkey: newStakeAccount.publicKey,
    authorized: new Authorized(authorized.publicKey, authorized.publicKey),
    lockup: new Lockup(0, 0, new PublicKey(0)),
    lamports: minimumAmount + 42,
  });

  await sendAndConfirmTransaction(
    connection,
    createAndInitialize,
    [authorized, newStakeAccount],
    {
      commitment: 'singleGossip',
    },
  );
  let delegation = StakeProgram.delegate({
    stakePubkey: newStakeAccount.publicKey,
    authorizedPubkey: authorized.publicKey,
    votePubkey,
  });
  await sendAndConfirmTransaction(connection, delegation, [authorized], {
    commitment: 'singleGossip',
  });

  const LARGE_EPOCH = 4000;
  await expect(
    connection.getStakeActivation(
      newStakeAccount.publicKey,
      'singleGossip',
      LARGE_EPOCH,
    ),
  ).rejects.toThrow(
    `failed to get Stake Activation ${newStakeAccount.publicKey.toBase58()}: Invalid param: epoch ${LARGE_EPOCH} has not yet started`,
  );

  const activationState = await connection.getStakeActivation(
    newStakeAccount.publicKey,
  );
  expect(activationState.state).toBe('activating');
  expect(activationState.inactive).toBe(42);
  expect(activationState.active).toBe(0);
});

test('stake activation should only accept state with valid string literals', async () => {
  if (!mockRpcEnabled) {
    console.log('live test skipped');
    return;
  }

  const connection = new Connection(url, 'singleGossip');
  const publicKey = new Account().publicKey;

  const addStakeActivationMock = state => {
    mockRpc.push([
      url,
      {
        method: 'getStakeActivation',
        params: [publicKey.toBase58(), {}],
      },
      {
        error: undefined,
        result: {
          state: state,
          active: 0,
          inactive: 80,
        },
      },
    ]);
  };

  addStakeActivationMock('active');
  let activation = await connection.getStakeActivation(publicKey);
  expect(activation.state).toBe('active');
  expect(activation.active).toBe(0);
  expect(activation.inactive).toBe(80);

  addStakeActivationMock('invalid');
  await expect(connection.getStakeActivation(publicKey)).rejects.toThrow();
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
  const connection = new Connection(url, 'singleGossip');

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [0, {commitment: 'singleGossip'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'singleGossip',
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

  mockConfirmTransaction(signature);
  await connection.confirmTransaction(signature);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [account.publicKey.toBase58(), {commitment: 'singleGossip'}],
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
      params: [
        account.publicKey.toBase58(),
        {commitment: 'singleGossip', encoding: 'base64'},
      ],
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
          data: ['', 'base64'],
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

  mockRpc.push([
    url,
    {
      method: 'getAccountInfo',
      params: [
        account.publicKey.toBase58(),
        {commitment: 'singleGossip', encoding: 'jsonParsed'},
      ],
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
          data: ['', 'base64'],
          executable: false,
        },
      },
    },
  ]);

  const parsedAccountInfo = (
    await connection.getParsedAccountInfo(account.publicKey)
  ).value;
  if (parsedAccountInfo === null) {
    expect(parsedAccountInfo).not.toBeNull();
    return;
  } else if (parsedAccountInfo.data.parsed) {
    expect(parsedAccountInfo.data.parsed).not.toBeTruthy();
    return;
  }
  expect(parsedAccountInfo.lamports).toBe(minimumAmount + 42);
  expect(parsedAccountInfo.data).toHaveLength(0);
  expect(parsedAccountInfo.owner).toEqual(SystemProgram.programId);
});

test('transaction failure', async () => {
  const account = new Account();
  const connection = new Connection(url, 'singleGossip');

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [0, {commitment: 'singleGossip'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'singleGossip',
  );

  mockRpc.push([
    url,
    {
      method: 'requestAirdrop',
      params: [account.publicKey.toBase58(), 3 * minimumAmount],
    },
    {
      error: null,
      result:
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  const airdropSignature = await connection.requestAirdrop(
    account.publicKey,
    3 * minimumAmount,
  );

  mockConfirmTransaction(airdropSignature);
  await connection.confirmTransaction(airdropSignature);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [account.publicKey.toBase58(), {commitment: 'singleGossip'}],
    },
    {
      error: null,
      result: {
        context: {
          slot: 11,
        },
        value: 3 * minimumAmount,
      },
    },
  ]);
  expect(await connection.getBalance(account.publicKey)).toBe(
    3 * minimumAmount,
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

  const newAccount = new Account();
  let transaction = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: account.publicKey,
      newAccountPubkey: newAccount.publicKey,
      lamports: minimumAmount,
      space: 0,
      programId: SystemProgram.programId,
    }),
  );

  mockConfirmTransaction(
    '3WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
  );
  await sendAndConfirmTransaction(
    connection,
    transaction,
    [account, newAccount],
    {commitment: 'singleGossip'},
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
  transaction = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: account.publicKey,
      newAccountPubkey: newAccount.publicKey,
      lamports: minimumAmount + 1,
      space: 0,
      programId: SystemProgram.programId,
    }),
  );
  const signature = await connection.sendTransaction(
    transaction,
    [account, newAccount],
    {skipPreflight: true},
  );

  const expectedErr = {InstructionError: [0, {Custom: 0}]};
  mockRpcSocket.push([
    {
      method: 'signatureSubscribe',
      params: [signature, {commitment: 'singleGossip'}],
    },
    {
      context: {
        slot: 11,
      },
      value: {err: expectedErr},
    },
  ]);

  // Wait for one confirmation
  const confirmResult = (
    await connection.confirmTransaction(signature, 'singleGossip')
  ).value;
  expect(confirmResult.err).toEqual(expectedErr);

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
  const connection = new Connection(url, 'singleGossip');

  mockRpc.push([
    url,
    {
      method: 'getMinimumBalanceForRentExemption',
      params: [0, {commitment: 'singleGossip'}],
    },
    {
      error: null,
      result: 50,
    },
  ]);

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'singleGossip',
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
        '2WE5w4B7v59x6qjyC4FbG2FEKYKQfvsJwqSxNVmtMjT8TQ31hsZieDHcSgqzxiAoTL56n2w5TncjqEKjLhtF4Vk',
    },
  ]);
  const airdropFromSig = await connection.requestAirdrop(
    accountFrom.publicKey,
    minimumAmount + 100010,
  );

  mockConfirmTransaction(airdropFromSig);
  await connection.confirmTransaction(airdropFromSig);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58(), {commitment: 'singleGossip'}],
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

  const airdropToSig = await connection.requestAirdrop(
    accountTo.publicKey,
    minimumAmount,
  );

  mockConfirmTransaction(airdropToSig);
  await connection.confirmTransaction(airdropToSig);

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountTo.publicKey.toBase58(), {commitment: 'singleGossip'}],
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

  const transaction = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: accountFrom.publicKey,
      toPubkey: accountTo.publicKey,
      lamports: 10,
    }),
  );
  const signature = await connection.sendTransaction(
    transaction,
    [accountFrom],
    {skipPreflight: true},
  );

  mockConfirmTransaction(signature);
  let confirmResult = (
    await connection.confirmTransaction(signature, 'singleGossip')
  ).value;
  expect(confirmResult.err).toBeNull();

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
  const transaction2 = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: accountFrom.publicKey,
      toPubkey: accountTo.publicKey,
      lamports: 10,
    }),
  );
  const signature2 = await connection.sendTransaction(
    transaction2,
    [accountFrom],
    {skipPreflight: true},
  );
  expect(signature).not.toEqual(signature2);
  expect(transaction.recentBlockhash).not.toEqual(transaction2.recentBlockhash);

  mockConfirmTransaction(signature2);
  await connection.confirmTransaction(signature2, 'singleGossip');

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
  const transaction3 = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: accountFrom.publicKey,
      toPubkey: accountTo.publicKey,
      lamports: 9,
    }),
  );
  const signature3 = await connection.sendTransaction(
    transaction3,
    [accountFrom],
    {
      skipPreflight: true,
    },
  );
  expect(transaction2.recentBlockhash).toEqual(transaction3.recentBlockhash);

  mockConfirmTransaction(signature3);
  await connection.confirmTransaction(signature3, 'singleGossip');

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

  const transaction4 = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: accountFrom.publicKey,
      toPubkey: accountTo.publicKey,
      lamports: 13,
    }),
  );

  const signature4 = await connection.sendTransaction(
    transaction4,
    [accountFrom],
    {
      skipPreflight: true,
    },
  );
  mockConfirmTransaction(signature4);
  await connection.confirmTransaction(signature4, 'singleGossip');

  expect(transaction4.recentBlockhash).not.toEqual(
    transaction3.recentBlockhash,
  );

  mockRpc.push([
    url,
    {
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58(), {commitment: 'singleGossip'}],
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
      params: [accountTo.publicKey.toBase58(), {commitment: 'singleGossip'}],
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
  const connection = new Connection(url, 'singleGossip');

  let signature = await connection.requestAirdrop(
    accountFrom.publicKey,
    LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature, 'singleGossip');
  expect(await connection.getBalance(accountFrom.publicKey)).toBe(
    LAMPORTS_PER_SOL,
  );

  const minimumAmount = await connection.getMinimumBalanceForRentExemption(
    0,
    'singleGossip',
  );

  signature = await connection.requestAirdrop(
    accountTo.publicKey,
    minimumAmount + 21,
  );
  await connection.confirmTransaction(signature);
  expect(await connection.getBalance(accountTo.publicKey)).toBe(
    minimumAmount + 21,
  );

  // 1. Move(accountFrom, accountTo)
  // 2. Move(accountTo, accountFrom)
  const transaction = new Transaction()
    .add(
      SystemProgram.transfer({
        fromPubkey: accountFrom.publicKey,
        toPubkey: accountTo.publicKey,
        lamports: 100,
      }),
    )
    .add(
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

  await connection.confirmTransaction(signature, 'singleGossip');

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

  const connection = new Connection(url, 'singleGossip');
  const owner = new Account();
  const programAccount = new Account();

  const mockCallback = jest.fn();

  const subscriptionId = connection.onAccountChange(
    programAccount.publicKey,
    mockCallback,
    'singleGossip',
  );

  const balanceNeeded = Math.max(
    await connection.getMinimumBalanceForRentExemption(0),
    1,
  );

  let signature = await connection.requestAirdrop(
    owner.publicKey,
    LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature);
  try {
    const transaction = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: owner.publicKey,
        toPubkey: programAccount.publicKey,
        lamports: balanceNeeded,
      }),
    );
    await sendAndConfirmTransaction(connection, transaction, [owner], {
      commitment: 'singleGossip',
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

  const connection = new Connection(url, 'singleGossip');
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

  let signature = await connection.requestAirdrop(
    owner.publicKey,
    LAMPORTS_PER_SOL,
  );
  await connection.confirmTransaction(signature);
  try {
    const transaction = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: owner.publicKey,
        toPubkey: programAccount.publicKey,
        lamports: balanceNeeded,
      }),
    );
    await sendAndConfirmTransaction(connection, transaction, [owner], {
      commitment: 'singleGossip',
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

  const connection = new Connection(url, 'singleGossip');

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

  const connection = new Connection(url, 'singleGossip');

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

test('https request', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const connection = new Connection('https://devnet.solana.com');
  const version = await connection.getVersion();
  expect(version['solana-core']).toBeTruthy();
});
