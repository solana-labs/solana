import bs58 from 'bs58';
import {Buffer} from 'buffer';
import * as splToken from '@solana/spl-token';
import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';
import {Agent as HttpAgent} from 'http';
import {Agent as HttpsAgent} from 'https';
import {match, mock, spy, useFakeTimers, SinonFakeTimers} from 'sinon';
import sinonChai from 'sinon-chai';
import {fail} from 'assert';

import {
  Authorized,
  Connection,
  EpochSchedule,
  SystemProgram,
  Transaction,
  LAMPORTS_PER_SOL,
  Lockup,
  PublicKey,
  StakeProgram,
  sendAndConfirmTransaction,
  Keypair,
  Message,
  AddressLookupTableProgram,
  SYSTEM_INSTRUCTION_LAYOUTS,
  NONCE_ACCOUNT_LENGTH,
  MessageAddressTableLookup,
} from '../src';
import invariant from '../src/utils/assert';
import {toBuffer} from '../src/utils/to-buffer';
import {MOCK_PORT, url, Node14Controller, nodeVersion} from './url';
import {
  AccountInfo,
  BLOCKHASH_CACHE_TIMEOUT_MS,
  BlockResponse,
  BlockSignatures,
  Commitment,
  ConfirmedBlock,
  Context,
  EpochInfo,
  InflationGovernor,
  InflationRate,
  Logs,
  SignatureResult,
  SlotInfo,
} from '../src/connection';
import {sleep} from '../src/utils/sleep';
import {
  helpers,
  mockErrorMessage,
  mockErrorResponse,
  mockRpcBatchResponse,
  mockRpcResponse,
  mockServer,
} from './mocks/rpc-http';
import {
  stubRpcWebSocket,
  restoreRpcWebSocket,
  mockRpcMessage,
} from './mocks/rpc-websockets';
import {
  NonceInformation,
  TransactionInstruction,
  TransactionSignature,
  TransactionExpiredBlockheightExceededError,
  TransactionExpiredNonceInvalidError,
  TransactionExpiredTimeoutError,
  TransactionMessage,
} from '../src/transaction';
import type {
  SignatureStatus,
  TransactionError,
  KeyedAccountInfo,
} from '../src/connection';
import {VersionedTransaction} from '../src/transaction/versioned';
import {MessageV0} from '../src/message/v0';
import {encodeData} from '../src/instruction';

use(chaiAsPromised);
use(sinonChai);

async function mockNonceAccountResponse(
  nonceAccountPubkey: string,
  nonceValue: string,
  nonceAuthority: string,
  slot?: number,
) {
  const mockNonceAccountData = Buffer.alloc(NONCE_ACCOUNT_LENGTH);
  mockNonceAccountData.fill(0);
  // Authority starts after 4 version bytes and 4 state bytes.
  mockNonceAccountData.set(bs58.decode(nonceAuthority), 4 + 4);
  // Nonce hash starts 32 bytes after the authority.
  mockNonceAccountData.set(bs58.decode(nonceValue), 4 + 4 + 32);
  await mockRpcResponse({
    method: 'getAccountInfo',
    params: [nonceAccountPubkey, {encoding: 'base64'}],
    value: {
      owner: SystemProgram.programId.toBase58(),
      lamports: LAMPORTS_PER_SOL,
      data: [mockNonceAccountData.toString('base64'), 'base64'],
      executable: false,
      rentEpoch: 20,
    },
    slot,
    withContext: true,
  });
}

const verifySignatureStatus = (
  status: SignatureStatus | null,
  err?: TransactionError,
): SignatureStatus => {
  if (status === null) {
    expect(status).not.to.be.null;
    throw new Error(); // unreachable
  }

  const expectedErr = err || null;
  expect(status.err).to.eql(expectedErr);
  expect(status.slot).to.be.at.least(0);
  if (expectedErr !== null) return status;

  const confirmations = status.confirmations;
  if (typeof confirmations === 'number') {
    expect(confirmations).to.be.at.least(0);
  } else {
    expect(confirmations).to.be.null;
  }
  return status;
};

describe('Connection', function () {
  let connection: Connection;
  beforeEach(() => {
    connection = new Connection(url);
  });

  if (mockServer) {
    const server = mockServer;
    beforeEach(() => {
      server.start(MOCK_PORT);
      stubRpcWebSocket(connection);
    });

    afterEach(() => {
      server.stop();
      restoreRpcWebSocket(connection);
    });
  }

  if (mockServer) {
    it('should pass HTTP headers to RPC', async () => {
      const headers = {
        Authorization: 'Bearer 123',
      };

      let connection = new Connection(url, {
        httpHeaders: headers,
      });

      await mockRpcResponse({
        method: 'getVersion',
        params: [],
        value: {'solana-core': '0.20.4'},
        withHeaders: headers,
      });

      expect(await connection.getVersion()).to.be.not.null;
    });

    it('should allow middleware to augment request', async () => {
      let connection = new Connection(url, {
        fetchMiddleware: (url, options, fetch) => {
          if (options) {
            options.headers = Object.assign(options.headers!, {
              Authorization: 'Bearer 123',
            });

            fetch(url, options);
          } else {
            fail('options must be defined!');
          }
        },
      });

      await mockRpcResponse({
        method: 'getVersion',
        params: [],
        value: {'solana-core': '0.20.4'},
        withHeaders: {
          Authorization: 'Bearer 123',
        },
      });

      expect(await connection.getVersion()).to.be.not.null;
    });
  }

  describe('override HTTP agent', () => {
    let previousBrowserEnv: string | undefined;
    beforeEach(() => {
      previousBrowserEnv = process.env.BROWSER;
      delete process.env.BROWSER;
    });
    afterEach(() => {
      process.env.BROWSER = previousBrowserEnv;
    });

    it('uses no agent with fetch when `overrideAgent` is `false`', () => {
      const fetch = spy();
      const c = new Connection(url, {httpAgent: false, fetch});
      c.getBlock(0);
      expect(fetch).to.have.been.calledWith(
        match.any,
        match({agent: undefined}),
      );
    });

    it('uses the supplied `overrideAgent` with fetch', () => {
      const fetch = spy();
      const httpAgent = new HttpsAgent();
      const c = new Connection('https://example.com', {httpAgent, fetch});
      c.getBlock(0);
      expect(fetch).to.have.been.calledWith(
        match.any,
        match({agent: httpAgent}),
      );
    });

    it('throws when the supplied `overrideAgent` is http but the endpoint is https', () => {
      expect(() => {
        new Connection('https://example.com', {httpAgent: new HttpAgent()});
      }).to.throw;
    });

    it('throws when the supplied `overrideAgent` is https but the endpoint is http', () => {
      expect(() => {
        new Connection('http://example.com', {httpAgent: new HttpsAgent()});
      }).to.throw;
    });
  });

  it('should attribute middleware fatals to the middleware', async () => {
    let connection = new Connection(url, {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      fetchMiddleware: (_url, _options, _fetch) => {
        throw new Error('This middleware experienced a fatal error');
      },
    });
    const error = await expect(connection.getVersion()).to.be.rejectedWith(
      'This middleware experienced a fatal error',
    );
    expect(error)
      .to.be.an.instanceOf(Error)
      .and.to.have.property('stack')
      .that.include('fetchMiddleware');
  });

  it('should not attribute fetch errors to the middleware', async () => {
    let connection = new Connection(url, {
      fetchMiddleware: (url, _options, fetch) => {
        fetch(url, {
          body: 'An `Object` was expected here; this is a `TypeError`.',
        });
      },
    });
    const error = await expect(connection.getVersion()).to.be.rejected;
    expect(error)
      .to.be.an.instanceOf(Error)
      .and.to.have.property('stack')
      .that.does.not.include('fetchMiddleware');
  });

  it('get account info - not found', async () => {
    const account = Keypair.generate();

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58(), {encoding: 'base64'}],
      value: null,
      withContext: true,
    });

    expect(await connection.getAccountInfo(account.publicKey)).to.be.null;

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [account.publicKey.toBase58(), {encoding: 'jsonParsed'}],
      value: null,
      withContext: true,
    });

    expect((await connection.getParsedAccountInfo(account.publicKey)).value).to
      .be.null;
  });

  it('get multiple accounts info', async () => {
    const account1 = Keypair.generate();
    const account2 = Keypair.generate();

    {
      await helpers.airdrop({
        connection,
        address: account1.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      await helpers.airdrop({
        connection,
        address: account2.publicKey,
        amount: LAMPORTS_PER_SOL,
      });
    }

    const value = [
      {
        owner: '11111111111111111111111111111111',
        lamports: LAMPORTS_PER_SOL,
        data: ['', 'base64'],
        executable: false,
        rentEpoch: 0,
        space: 0,
      },
      {
        owner: '11111111111111111111111111111111',
        lamports: LAMPORTS_PER_SOL,
        data: ['', 'base64'],
        executable: false,
        rentEpoch: 0,
        space: 0,
      },
    ];

    await mockRpcResponse({
      method: 'getMultipleAccounts',
      params: [
        [account1.publicKey.toBase58(), account2.publicKey.toBase58()],
        {encoding: 'base64'},
      ],
      value: value,
      withContext: true,
    });

    const res = await connection.getMultipleAccountsInfo(
      [account1.publicKey, account2.publicKey],
      'confirmed',
    );

    const expectedValue = [
      {
        owner: new PublicKey('11111111111111111111111111111111'),
        lamports: LAMPORTS_PER_SOL,
        data: Buffer.from([]),
        executable: false,
        rentEpoch: 0,
        space: 0,
      },
      {
        owner: new PublicKey('11111111111111111111111111111111'),
        lamports: LAMPORTS_PER_SOL,
        data: Buffer.from([]),
        executable: false,
        rentEpoch: 0,
        space: 0,
      },
    ];

    expect(res).to.eql(expectedValue);
  });

  it('get program accounts', async () => {
    const account0 = Keypair.generate();
    const account1 = Keypair.generate();
    const programId = Keypair.generate();

    {
      await helpers.airdrop({
        connection,
        address: account0.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      const transaction = new Transaction().add(
        SystemProgram.assign({
          accountPubkey: account0.publicKey,
          programId: programId.publicKey,
        }),
      );

      await helpers.processTransaction({
        connection,
        transaction,
        signers: [account0],
        commitment: 'confirmed',
      });
    }

    {
      await helpers.airdrop({
        connection,
        address: account1.publicKey,
        amount: 0.5 * LAMPORTS_PER_SOL,
      });

      const transaction = new Transaction().add(
        SystemProgram.assign({
          accountPubkey: account1.publicKey,
          programId: programId.publicKey,
        }),
      );

      await helpers.processTransaction({
        connection,
        transaction,
        signers: [account1],
        commitment: 'confirmed',
      });
    }

    const feeCalculator = (await helpers.recentBlockhash({connection}))
      .feeCalculator;

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {commitment: 'confirmed', encoding: 'base64'},
        ],
        value: [
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
      });

      const programAccounts = await connection.getProgramAccounts(
        programId.publicKey,
        {
          commitment: 'confirmed',
        },
      );
      expect(programAccounts).to.have.length(2);
      programAccounts.forEach(function (keyedAccount) {
        if (keyedAccount.pubkey.equals(account0.publicKey)) {
          expect(keyedAccount.account.lamports).to.eq(
            LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        } else {
          expect(keyedAccount.pubkey).to.eql(account1.publicKey);
          expect(keyedAccount.account.lamports).to.eq(
            0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        }
      });
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {commitment: 'confirmed', encoding: 'base64'},
        ],
        value: [
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
      });

      const programAccounts = await connection.getProgramAccounts(
        programId.publicKey,
        'confirmed',
      );
      expect(programAccounts).to.have.length(2);
      programAccounts.forEach(function (keyedAccount) {
        if (keyedAccount.pubkey.equals(account0.publicKey)) {
          expect(keyedAccount.account.lamports).to.eq(
            LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        } else {
          expect(keyedAccount.pubkey).to.eql(account1.publicKey);
          expect(keyedAccount.account.lamports).to.eq(
            0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        }
      });
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {
            commitment: 'confirmed',
            encoding: 'base64',
            filters: [
              {
                dataSize: 0,
              },
            ],
          },
        ],
        value: [
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
      });

      const programAccountsDoMatchFilter = await connection.getProgramAccounts(
        programId.publicKey,
        {
          commitment: 'confirmed',
          encoding: 'base64',
          filters: [{dataSize: 0}],
        },
      );
      expect(programAccountsDoMatchFilter).to.have.length(2);
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {
            commitment: 'confirmed',
            encoding: 'base64',
            filters: [
              {
                memcmp: {
                  offset: 0,
                  bytes: 'XzdZ3w',
                },
              },
            ],
          },
        ],
        value: [],
      });

      const programAccountsDontMatchFilter =
        await connection.getProgramAccounts(programId.publicKey, {
          commitment: 'confirmed',
          filters: [
            {
              memcmp: {
                offset: 0,
                bytes: 'XzdZ3w',
              },
            },
          ],
        });
      expect(programAccountsDontMatchFilter).to.have.length(0);
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {commitment: 'confirmed', encoding: 'jsonParsed'},
        ],
        value: [
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
      });

      const programAccounts = await connection.getParsedProgramAccounts(
        programId.publicKey,
        {
          commitment: 'confirmed',
        },
      );
      expect(programAccounts).to.have.length(2);

      programAccounts.forEach(function (element) {
        if (element.pubkey.equals(account0.publicKey)) {
          expect(element.account.lamports).to.eq(
            LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        } else {
          expect(element.pubkey).to.eql(account1.publicKey);
          expect(element.account.lamports).to.eq(
            0.5 * LAMPORTS_PER_SOL - feeCalculator.lamportsPerSignature,
          );
        }
      });
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {
            commitment: 'confirmed',
            encoding: 'jsonParsed',
            filters: [
              {
                dataSize: 2,
              },
            ],
          },
        ],
        value: [],
      });

      const programAccountsDontMatchFilter =
        await connection.getParsedProgramAccounts(programId.publicKey, {
          commitment: 'confirmed',
          filters: [{dataSize: 2}],
        });
      expect(programAccountsDontMatchFilter).to.have.length(0);
    }

    {
      await mockRpcResponse({
        method: 'getProgramAccounts',
        params: [
          programId.publicKey.toBase58(),
          {
            commitment: 'confirmed',
            encoding: 'jsonParsed',
            filters: [
              {
                memcmp: {
                  offset: 0,
                  bytes: '',
                },
              },
            ],
          },
        ],
        value: [
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
      });

      const programAccountsDoMatchFilter =
        await connection.getParsedProgramAccounts(programId.publicKey, {
          commitment: 'confirmed',
          filters: [
            {
              memcmp: {
                offset: 0,
                bytes: '',
              },
            },
          ],
        });
      expect(programAccountsDoMatchFilter).to.have.length(2);
    }
  }).timeout(30 * 1000);

  it('get balance', async () => {
    const account = Keypair.generate();

    await mockRpcResponse({
      method: 'getBalance',
      params: [account.publicKey.toBase58()],
      value: {
        context: {
          slot: 11,
        },
        value: 0,
      },
    });

    const balance = await connection.getBalance(account.publicKey);
    expect(balance).to.be.at.least(0);
  });

  it('get inflation', async () => {
    await mockRpcResponse({
      method: 'getInflationGovernor',
      params: [],
      value: {
        foundation: 0.05,
        foundationTerm: 7.0,
        initial: 0.15,
        taper: 0.15,
        terminal: 0.015,
      },
    });

    const inflation = await connection.getInflationGovernor();
    const inflationKeys: (keyof InflationGovernor)[] = [
      'initial',
      'terminal',
      'taper',
      'foundation',
      'foundationTerm',
    ];

    for (const key of inflationKeys) {
      expect(inflation).to.have.property(key);
      expect(inflation[key]).to.be.greaterThan(0);
    }
  });

  it('get inflation reward', async () => {
    if (mockServer) {
      await mockRpcResponse({
        method: 'getInflationReward',
        params: [
          [
            '7GHnTRB8Rz14qZQhDXf8ox1Kfu7mPcPLpKaBJJirmYj2',
            'CrinLuHjVGDDcQfrEoCmM4k31Ni9sMoTCEEvNSUSh7Jg',
          ],
          {
            epoch: 0,
          },
        ],
        value: [
          {
            amount: 3646143,
            effectiveSlot: 432000,
            epoch: 0,
            postBalance: 30504783,
            commission: 0,
          },
          null,
        ],
      });

      const inflationReward = await connection.getInflationReward(
        [
          new PublicKey('7GHnTRB8Rz14qZQhDXf8ox1Kfu7mPcPLpKaBJJirmYj2'),
          new PublicKey('CrinLuHjVGDDcQfrEoCmM4k31Ni9sMoTCEEvNSUSh7Jg'),
        ],
        0,
      );

      expect(inflationReward).to.have.lengthOf(2);
    }
  });

  it('get inflation rate', async () => {
    await mockRpcResponse({
      method: 'getInflationRate',
      params: [],
      value: {
        total: 0.08,
        validator: 0.076,
        foundation: 0.004,
        epoch: 1,
      },
    });

    const inflation = await connection.getInflationRate();
    const inflationKeys: (keyof InflationRate)[] = [
      'total',
      'validator',
      'foundation',
      'epoch',
    ];

    for (const key of inflationKeys) {
      expect(inflation).to.have.property(key);
      if (mockServer) {
        expect(inflation[key]).to.be.greaterThan(0);
      } else {
        expect(inflation[key]).to.be.at.least(0);
      }
    }
  });

  it('get epoch info', async () => {
    await mockRpcResponse({
      method: 'getEpochInfo',
      params: [{commitment: 'confirmed'}],
      value: {
        epoch: 0,
        slotIndex: 1,
        slotsInEpoch: 8192,
        absoluteSlot: 1,
        blockHeight: 1,
      },
    });

    const epochInfo = await connection.getEpochInfo('confirmed');
    const epochInfoKeys: (keyof EpochInfo)[] = [
      'epoch',
      'slotIndex',
      'slotsInEpoch',
      'absoluteSlot',
      'blockHeight',
    ];

    for (const key of epochInfoKeys) {
      expect(epochInfo).to.have.property(key);
      expect(epochInfo[key]).to.be.at.least(0);
    }
  });

  it('get epoch schedule', async () => {
    await mockRpcResponse({
      method: 'getEpochSchedule',
      params: [],
      value: {
        firstNormalEpoch: 8,
        firstNormalSlot: 8160,
        leaderScheduleSlotOffset: 8192,
        slotsPerEpoch: 8192,
        warmup: true,
      },
    });

    const epochSchedule = await connection.getEpochSchedule();
    const epochScheduleKeys: (keyof EpochSchedule)[] = [
      'firstNormalEpoch',
      'firstNormalSlot',
      'leaderScheduleSlotOffset',
      'slotsPerEpoch',
    ];

    for (const key of epochScheduleKeys) {
      expect(epochSchedule).to.have.property('warmup');
      expect(epochSchedule).to.have.property(key);
      if (epochSchedule.warmup) {
        expect(epochSchedule[key]).to.be.greaterThan(0);
      }
    }
  });

  it('get leader schedule', async () => {
    await mockRpcResponse({
      method: 'getLeaderSchedule',
      params: [],
      value: {
        '123vij84ecQEKUvQ7gYMKxKwKF6PbYSzCzzURYA4xULY': [0, 1, 2, 3],
        '8PTjAikKoAybKXcEPnDSoy8wSNNikUBJ1iKawJKQwXnB': [4, 5, 6, 7],
      },
    });

    const leaderSchedule = await connection.getLeaderSchedule();
    expect(Object.keys(leaderSchedule).length).to.be.at.least(1);
    for (const key in leaderSchedule) {
      const slots = leaderSchedule[key];
      expect(Array.isArray(slots)).to.be.true;
      expect(slots.length).to.be.at.least(4);
    }
  });

  it('get slot', async () => {
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 123,
    });

    const slot = await connection.getSlot();
    if (mockServer) {
      expect(slot).to.eq(123);
    } else {
      // No idea what the correct slot value should be on a live cluster, so
      // just check the type
      expect(typeof slot).to.eq('number');
    }
  });

  it('get slot leader', async () => {
    await mockRpcResponse({
      method: 'getSlotLeader',
      params: [],
      value: '11111111111111111111111111111111',
    });

    const slotLeader = await connection.getSlotLeader();
    if (mockServer) {
      expect(slotLeader).to.eq('11111111111111111111111111111111');
    } else {
      // No idea what the correct slotLeader value should be on a live cluster, so
      // just check the type
      expect(typeof slotLeader).to.eq('string');
    }
  });

  it('get slot leaders', async () => {
    await mockRpcResponse({
      method: 'getSlotLeaders',
      params: [0, 1],
      value: ['11111111111111111111111111111111'],
    });

    const slotLeaders = await connection.getSlotLeaders(0, 1);
    expect(slotLeaders).to.have.length(1);
    expect(slotLeaders[0]).to.be.instanceOf(PublicKey);
  });

  it('get cluster nodes', async () => {
    await mockRpcResponse({
      method: 'getClusterNodes',
      params: [],
      value: [
        {
          pubkey: '11111111111111111111111111111111',
          gossip: '127.0.0.0:1234',
          tpu: '127.0.0.0:1235',
          rpc: null,
          version: '1.1.10',
        },
      ],
    });

    const clusterNodes = await connection.getClusterNodes();
    if (mockServer) {
      expect(clusterNodes).to.have.length(1);
      expect(clusterNodes[0].pubkey).to.eq('11111111111111111111111111111111');
      expect(typeof clusterNodes[0].gossip).to.eq('string');
      expect(typeof clusterNodes[0].tpu).to.eq('string');
      expect(clusterNodes[0].rpc).to.be.null;
    } else {
      // There should be at least one node (the node that we're talking to)
      expect(clusterNodes.length).to.be.greaterThan(0);
    }
  });

  if (process.env.TEST_LIVE) {
    it('get vote accounts', async () => {
      const voteAccounts = await connection.getVoteAccounts();
      expect(
        voteAccounts.current.concat(voteAccounts.delinquent).length,
      ).to.be.greaterThan(0);
    });
  }

  if (process.env.TEST_LIVE) {
    describe('transaction confirmation (live)', () => {
      let connection: Connection;
      beforeEach(() => {
        connection = new Connection(url, 'confirmed');
      });

      describe('blockheight based transaction confirmation', () => {
        let latestBlockhash: {blockhash: string; lastValidBlockHeight: number};
        let signature: string;

        beforeEach(async function () {
          this.timeout(60 * 1000);
          const keypair = Keypair.generate();
          const [
            // eslint-disable-next-line @typescript-eslint/no-unused-vars
            _,
            blockhash,
          ] = await Promise.all([
            connection.confirmTransaction(
              await connection.requestAirdrop(
                keypair.publicKey,
                LAMPORTS_PER_SOL,
              ),
            ),
            helpers.latestBlockhash({connection}),
          ]);
          latestBlockhash = blockhash;
          const ix = new TransactionInstruction({
            keys: [
              {
                pubkey: keypair.publicKey,
                isSigner: true,
                isWritable: true,
              },
            ],
            programId: new PublicKey(
              'MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr',
            ),
            data: Buffer.from('Hello world', 'utf8'),
          });

          const transaction = new Transaction({
            ...latestBlockhash,
          });
          transaction.add(ix);
          transaction.sign(keypair);
          signature = await connection.sendTransaction(transaction, [keypair]);
        });

        it('confirms transactions using the last valid blockheight strategy', async () => {
          let result = await connection.confirmTransaction(
            {
              signature,
              ...latestBlockhash,
            },
            'processed',
          );
          expect(result.value).to.have.property('err', null);
        }).timeout(60 * 1000);

        it('throws when confirming using a blockhash whose last valid blockheight has passed', async () => {
          const confirmationPromise = connection.confirmTransaction({
            signature,
            ...latestBlockhash,
            lastValidBlockHeight: (await connection.getBlockHeight()) - 1, // Simulate the blockheight having passed.
          });
          expect(confirmationPromise).to.eventually.be.rejectedWith(
            TransactionExpiredBlockheightExceededError,
          );
        }).timeout(60 * 1000);
      });

      describe('nonce-based transaction confirmation', () => {
        let keypair: Keypair;
        let minContextSlot: number;
        let nonceInfo: NonceInformation;
        let nonceKeypair: Keypair;
        let transaction: Transaction;

        beforeEach(async function () {
          this.timeout(60 * 1000);
          keypair = Keypair.generate();
          nonceKeypair = Keypair.generate();
          const [
            // eslint-disable-next-line @typescript-eslint/no-unused-vars
            _,
            blockhash,
            minimumNonceAccountRentLamports,
          ] = await Promise.all([
            connection.confirmTransaction(
              await connection.requestAirdrop(
                keypair.publicKey,
                LAMPORTS_PER_SOL,
              ),
            ),
            helpers.latestBlockhash({connection}),
            connection.getMinimumBalanceForRentExemption(NONCE_ACCOUNT_LENGTH),
          ]);
          const createNonceAccountTransaction =
            SystemProgram.createNonceAccount({
              authorizedPubkey: keypair.publicKey,
              fromPubkey: keypair.publicKey,
              lamports: minimumNonceAccountRentLamports,
              noncePubkey: nonceKeypair.publicKey,
            });
          createNonceAccountTransaction.recentBlockhash = blockhash.blockhash;
          createNonceAccountTransaction.feePayer = keypair.publicKey;
          const createNonceAccountTransactionSignature =
            await connection.sendTransaction(createNonceAccountTransaction, [
              keypair,
              nonceKeypair,
            ]);
          const {context} = await connection.confirmTransaction({
            ...blockhash,
            signature: createNonceAccountTransactionSignature,
          });
          minContextSlot = context.slot;
          const nonceAccount = await connection.getNonce(
            nonceKeypair.publicKey,
            {minContextSlot},
          );
          nonceInfo = {
            nonce: nonceAccount!.nonce,
            nonceInstruction: SystemProgram.nonceAdvance({
              authorizedPubkey: keypair.publicKey,
              noncePubkey: nonceKeypair.publicKey,
            }),
          };
          invariant(
            nonceAccount,
            'Expected a nonce account to have been created in the test setup',
          );
          const ix = new TransactionInstruction({
            keys: [
              {
                pubkey: keypair.publicKey,
                isSigner: true,
                isWritable: true,
              },
            ],
            programId: new PublicKey(
              'MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr',
            ),
            data: Buffer.from('Hello world', 'utf8'),
          });
          transaction = new Transaction({minContextSlot, nonceInfo});
          transaction.add(ix);
          transaction.sign(keypair);
        });

        it('confirms transactions using the durable nonce strategy', async () => {
          const signature = await connection.sendTransaction(transaction, [
            keypair,
          ]);
          const result = await connection.confirmTransaction(
            {
              minContextSlot,
              nonceAccountPubkey: nonceKeypair.publicKey,
              nonceValue: nonceInfo.nonce,
              signature,
            },
            'processed',
          );
          expect(result.value).to.have.property('err', null);
        }).timeout(60 * 1000);

        it('throws when confirming using a nonce that is no longer valid', async () => {
          // Advance the nonce.
          const blockhash = await connection.getLatestBlockhash();
          await sendAndConfirmTransaction(
            connection,
            new Transaction({feePayer: keypair.publicKey, ...blockhash}).add(
              nonceInfo.nonceInstruction,
            ),
            [keypair],
          );
          const [currentSlot, signature] = await Promise.all([
            connection.getSlot(),
            connection.sendTransaction(transaction, [keypair], {
              skipPreflight: true,
            }),
          ]);
          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: currentSlot,
            nonceAccountPubkey: nonceKeypair.publicKey,
            nonceValue: nonceInfo.nonce, // The old nonce.
            signature,
          });
          await expect(confirmationPromise).to.eventually.be.rejectedWith(
            TransactionExpiredNonceInvalidError,
          );
        }).timeout(60 * 1000);
      });
    });
  }

  if (!process.env.TEST_LIVE) {
    describe('transaction confirmation (mock)', () => {
      let clock: SinonFakeTimers;
      beforeEach(() => {
        clock = useFakeTimers();
      });

      afterEach(() => {
        clock.restore();
      });

      describe('timeout strategy (deprecated)', () => {
        it('throws a `TransactionExpiredTimeoutError` when the timer elapses without a signature confirmation', async () => {
          const mockSignature =
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}),
          });
          const timeoutPromise = connection.confirmTransaction(mockSignature);

          // Advance the clock past all waiting timers, notably the expiry timer.
          clock.runAllAsync();

          await expect(timeoutPromise).to.be.rejectedWith(
            TransactionExpiredTimeoutError,
          );
        });
      });

      describe('block height strategy', () => {
        it('rejects if called with an already-aborted `abortSignal`', () => {
          const mockSignature =
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';
          const abortController: any =
            nodeVersion >= 16 ? new AbortController() : Node14Controller();
          abortController.abort();
          expect(
            connection.confirmTransaction({
              abortSignal: abortController.signal,
              blockhash: 'sampleBlockhash',
              lastValidBlockHeight: 1,
              signature: mockSignature,
            }),
          ).to.eventually.be.rejectedWith('AbortError');
        });

        it('rejects upon receiving an abort signal', async () => {
          const mockSignature =
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';
          const abortController: any =
            nodeVersion >= 16 ? new AbortController() : Node14Controller();
          // Keep the subscription from ever returning data.
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve.
          });
          clock.runAllAsync();
          const confirmationPromise = connection.confirmTransaction({
            abortSignal: abortController.signal,
            blockhash: 'sampleBlockhash',
            lastValidBlockHeight: 1,
            signature: mockSignature,
          });
          clock.runAllAsync();
          expect(confirmationPromise).not.to.have.been.rejected;
          abortController.abort();
          await expect(confirmationPromise).to.eventually.be.rejected;
        });

        it('throws a `TransactionExpiredBlockheightExceededError` when the block height advances past the last valid one for this transaction without a signature confirmation', async () => {
          const mockSignature =
            '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve this = never get a response.
          });

          const lastValidBlockHeight = 3;

          // Start the block height at `lastValidBlockHeight - 1`.
          await mockRpcResponse({
            method: 'getBlockHeight',
            params: [],
            value: lastValidBlockHeight - 1,
          });

          const confirmationPromise = connection.confirmTransaction({
            signature: mockSignature,
            blockhash: 'sampleBlockhash',
            lastValidBlockHeight,
          });
          clock.runAllAsync();

          // Advance the block height to the `lastValidBlockHeight`.
          await mockRpcResponse({
            method: 'getBlockHeight',
            params: [],
            value: lastValidBlockHeight,
          });
          clock.runAllAsync();

          // Advance the block height to `lastValidBlockHeight + 1`,
          // past the last valid blockheight for this transaction.
          await mockRpcResponse({
            method: 'getBlockHeight',
            params: [],
            value: lastValidBlockHeight + 1,
          });
          clock.runAllAsync();
          await expect(confirmationPromise).to.be.rejectedWith(
            TransactionExpiredBlockheightExceededError,
          );
        });

        it('when the `getBlockHeight` method throws an error it does not timeout but rather keeps waiting for a confirmation', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          let resolveResultPromise = function (result: SignatureResult): any {
            return result;
          };

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise<SignatureResult>(resolve => {
              resolveResultPromise = resolve;
            }),
          });

          // Simulate a failure to fetch the block height.
          let rejectBlockheightPromise = function (): void {};
          await mockRpcResponse({
            method: 'getBlockHeight',
            params: [],
            value: (() => {
              const p = new Promise((_, reject) => {
                rejectBlockheightPromise = reject;
              });
              p.catch(() => {});
              return p;
            })(),
          });

          const confirmationPromise = connection.confirmTransaction({
            signature: mockSignature,
            blockhash: 'sampleBlockhash',
            lastValidBlockHeight: 3,
          });

          rejectBlockheightPromise();
          clock.runToLastAsync();
          resolveResultPromise({err: null});
          clock.runToLastAsync();

          expect(confirmationPromise).not.to.eventually.be.rejected;
        });

        it('confirms the transaction if the signature confirmation is received before the block height is exceeded', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          let resolveResultPromise = function (result: SignatureResult): any {
            return result;
          };
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise<SignatureResult>(resolve => {
              resolveResultPromise = resolve;
            }),
          });

          const lastValidBlockHeight = 3;

          // Advance the block height to the `lastValidBlockHeight`.
          await mockRpcResponse({
            method: 'getBlockHeight',
            params: [],
            value: lastValidBlockHeight,
          });

          const confirmationPromise = connection.confirmTransaction({
            signature: mockSignature,
            blockhash: 'sampleBlockhash',
            lastValidBlockHeight,
          });
          clock.runAllAsync();

          // Return a signature result in the nick of time.
          resolveResultPromise({err: null});

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });
      });

      describe('nonce strategy', () => {
        it('rejects if called with an already-aborted `abortSignal`', () => {
          const mockSignature =
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';
          const abortController: any =
            nodeVersion >= 16 ? new AbortController() : Node14Controller();
          abortController.abort();
          expect(
            connection.confirmTransaction({
              abortSignal: abortController.signal,
              minContextSlot: 1,
              nonceAccountPubkey: new PublicKey(1),
              nonceValue: 'fakenonce',
              signature: mockSignature,
            }),
          ).to.eventually.be.rejectedWith('AbortError');
        });

        it('rejects upon receiving an abort signal', async () => {
          const mockSignature =
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';
          const abortController: any =
            nodeVersion >= 16 ? new AbortController() : Node14Controller();
          // Keep the subscription from ever returning data.
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve.
          });
          clock.runAllAsync();
          const confirmationPromise = connection.confirmTransaction({
            abortSignal: abortController.signal,
            minContextSlot: 1,
            nonceAccountPubkey: new PublicKey(1),
            nonceValue: 'fakenonce',
            signature: mockSignature,
          });
          clock.runAllAsync();
          expect(confirmationPromise).not.to.have.been.rejected;
          abortController.abort();
          await expect(confirmationPromise).to.eventually.be.rejected;
        });

        it('confirms the transaction if the signature confirmation is received before the nonce is advanced', async () => {
          const mockSignature =
            '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG';

          let resolveResultPromise = function (result: SignatureResult): any {
            return result;
          };
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise<SignatureResult>(resolve => {
              resolveResultPromise = resolve;
            }),
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();
          const authority = new PublicKey(3);

          // Start with the nonce account matching the nonce used to sign the transaction.
          await mockNonceAccountResponse(
            nonceAccountPubkey.toBase58(),
            nonceValue,
            authority.toBase58(),
          );

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });
          clock.runAllAsync();

          // Respond, a second time, with the same nonce hash.
          await mockNonceAccountResponse(
            nonceAccountPubkey.toBase58(),
            nonceValue,
            authority.toBase58(),
          );
          clock.runAllAsync();

          // Return a signature result in the nick of time.
          resolveResultPromise({err: null});

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });

        it('succeeds if double-checking the signature after the nonce-advances demonstrates that the transaction is confirmed', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve this = never get a response.
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();
          const authority = new PublicKey(3);

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });

          // Simulate the nonce advancing but the double-check of the signature status succeeding.
          await mockNonceAccountResponse(
            nonceAccountPubkey.toBase58(),
            new PublicKey(4).toBase58(), // A new nonce.
            authority.toBase58(),
          );
          await mockRpcResponse({
            method: 'getSignatureStatuses',
            params: [[mockSignature]],
            value: [
              {
                err: null,
                confirmations: 0,
                confirmationStatus: 'finalized', // Demonstrate that the transaction is, in fact, confirmed.
                slot: 0,
              },
            ],
            withContext: true,
          });
          clock.runToLastAsync();

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });

        it('keeps double-checking the signature after the nonce-advances until a signature from the minimum allowable slot is obtained', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve this = never get a response.
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();
          const authority = new PublicKey(3);

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 11,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });

          // Simulate the nonce advancing but the double-check of the signature status succeeding.
          await mockNonceAccountResponse(
            nonceAccountPubkey.toBase58(),
            new PublicKey(4).toBase58(), // A new nonce.
            authority.toBase58(),
          );

          // Simulate getting a response from an old slot.
          await mockRpcResponse({
            method: 'getSignatureStatuses',
            params: [[mockSignature]],
            value: [
              {
                err: null,
                confirmations: 0,
                confirmationStatus: 'processed', // A non-finalized value from an old slot.
                slot: 10,
              },
            ],
            slot: 10,
            withContext: true,
          });

          // Then obtain a response from the minimum allowable slot.
          await mockRpcResponse({
            method: 'getSignatureStatuses',
            params: [[mockSignature]],
            value: [
              {
                err: null,
                confirmations: 32,
                confirmationStatus: 'finalized', // Demonstrate that the transaction is, in fact, confirmed.
                slot: 11,
              },
            ],
            slot: 11,
            withContext: true,
          });
          clock.runAllAsync();

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });

        it('throws a `TransactionExpiredNonceInvalidError` when the nonce is no longer the one with which this transaction was signed', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve this = never get a response.
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();
          const authority = new PublicKey(3);

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });

          // Simulate the nonce advancing but the double-check of the signature status succeeding.
          await mockNonceAccountResponse(
            nonceAccountPubkey.toBase58(),
            new PublicKey(4).toBase58(), // A new nonce.
            authority.toBase58(),
          );
          await mockRpcResponse({
            method: 'getSignatureStatuses',
            params: [[mockSignature]],
            value: [
              {
                err: null,
                confirmations: 0,
                confirmationStatus: 'processed', // Demonstrate that the transaction is, in fact, not confirmed.
                slot: 0,
              },
            ],
            withContext: true,
          });
          clock.runToLastAsync();

          await expect(confirmationPromise).to.eventually.be.rejectedWith(
            TransactionExpiredNonceInvalidError,
          );
        });

        it('when fetching the nonce account throws an error it does not timeout but rather keeps waiting for a confirmation', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          let resolveResultPromise = function (result: SignatureResult): any {
            return result;
          };
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise<SignatureResult>(resolve => {
              resolveResultPromise = resolve;
            }),
          });

          // Simulate a failure to fetch the nonce account.
          let rejectNonceAccountFetchPromise = function (): void {};
          await mockRpcResponse({
            method: 'getAccountInfo',
            params: [],
            value: (() => {
              const p = new Promise((_, reject) => {
                rejectNonceAccountFetchPromise = reject;
              });
              p.catch(() => {});
              return p;
            })(),
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });

          rejectNonceAccountFetchPromise();
          clock.runToLastAsync();
          resolveResultPromise({err: null});
          clock.runToLastAsync();

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });

        it('throws `TransactionExpiredNonceInvalidError` when the nonce account does not exist', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise(() => {}), // Never resolve this = never get a response.
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });

          // Simulate a non-existent nonce account.
          await mockRpcResponse({
            method: 'getAccountInfo',
            params: [],
            value: null,
            withContext: true,
          });
          clock.runToLastAsync();
          await mockRpcResponse({
            method: 'getSignatureStatuses',
            params: [[mockSignature]],
            value: [
              {
                err: null,
                confirmations: 0,
                confirmationStatus: 'processed', // Demonstrate that the transaction is, in fact, not confirmed.
                slot: 0,
              },
            ],
            withContext: true,
          });
          clock.runToLastAsync();

          await expect(confirmationPromise).to.eventually.be.rejectedWith(
            TransactionExpiredNonceInvalidError,
          );
        });

        it('when the nonce account data fails to deserialize', async () => {
          const mockSignature =
            'LPJ18iiyfz3G1LpNNbcBnBtaS4dVBdPHKrnELqikjER2DcvB4iyTgz43nKQJH3JQAJHuZdM1xVh5Cnc5Hc7LrqC';

          let resolveResultPromise = function (result: SignatureResult): any {
            return result;
          };
          await mockRpcMessage({
            method: 'signatureSubscribe',
            params: [mockSignature, {commitment: 'finalized'}],
            result: new Promise<SignatureResult>(resolve => {
              resolveResultPromise = resolve;
            }),
          });

          const nonceAccountPubkey = new PublicKey(1);
          const nonceValue = new PublicKey(2).toBase58();

          // Simulate a failure to deserialize the nonce.
          await mockRpcResponse({
            method: 'getAccountInfo',
            params: [nonceAccountPubkey.toBase58(), {encoding: 'base64'}],
            value: {
              owner: SystemProgram.programId.toBase58(),
              lamports: LAMPORTS_PER_SOL,
              data: ['JUNK_DATA', 'base64'],
              executable: false,
              rentEpoch: 20,
            },
            withContext: true,
          });

          const confirmationPromise = connection.confirmTransaction({
            minContextSlot: 0,
            nonceAccountPubkey,
            nonceValue,
            signature: mockSignature,
          });
          clock.runToLastAsync();

          resolveResultPromise({err: null});
          clock.runToLastAsync();

          await expect(confirmationPromise).to.eventually.deep.equal({
            context: {slot: 11},
            value: {err: null},
          });
        });
      });

      it('confirm transaction - does not check the signature status before the signature subscription comes alive', async () => {
        const mockSignature =
          'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';

        await mockRpcMessage({
          method: 'signatureSubscribe',
          params: [mockSignature, {commitment: 'finalized'}],
          result: {err: null},
          subscriptionEstablishmentPromise: new Promise(() => {}), // Never resolve.
        });
        const getSignatureStatusesExpectation = mock(connection)
          .expects('getSignatureStatuses')
          .never();
        connection.confirmTransaction(mockSignature);
        getSignatureStatusesExpectation.verify();
      });

      it('confirm transaction - checks the signature status once the signature subscription comes alive', async () => {
        const mockSignature =
          'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';

        await mockRpcMessage({
          method: 'signatureSubscribe',
          params: [mockSignature, {commitment: 'finalized'}],
          result: {err: null},
        });
        const getSignatureStatusesExpectation = mock(connection)
          .expects('getSignatureStatuses')
          .once();

        const confirmationPromise =
          connection.confirmTransaction(mockSignature);
        clock.runAllAsync();

        await expect(confirmationPromise).to.eventually.deep.equal({
          context: {slot: 11},
          value: {err: null},
        });
        getSignatureStatusesExpectation.verify();
      });

      // FIXME: This test does not work.
      // it('confirm transaction - confirms transaction when signature status check yields confirmation before signature subscription does', async () => {
      //   const mockSignature =
      //     'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';

      //   // Keep the subscription from ever returning data.
      //   await mockRpcMessage({
      //     method: 'signatureSubscribe',
      //     params: [mockSignature, {commitment: 'finalized'}],
      //     result: new Promise(() => {}), // Never resolve.
      //   });
      //   clock.runAllAsync();

      //   const confirmationPromise =
      //     connection.confirmTransaction(mockSignature);
      //   clock.runAllAsync();

      //   // Return a signature status through the RPC API.
      //   await mockRpcResponse({
      //     method: 'getSignatureStatuses',
      //     params: [[mockSignature]],
      //     value: [
      //       {
      //         slot: 0,
      //         confirmations: 11,
      //         status: {Ok: null},
      //         err: null,
      //       },
      //     ],
      //   });
      //   clock.runAllAsync();

      //   await expect(confirmationPromise).to.eventually.deep.equal({
      //     context: {slot: 11},
      //     value: {err: null},
      //   });
      // });

      it('confirm transaction - does not confirm the transaction when signature status check yields confirmation for a lower commitment before signature subscription confirms the transaction', async () => {
        const mockSignature =
          'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt';

        // Keep the subscription from ever returning data.
        await mockRpcMessage({
          method: 'signatureSubscribe',
          params: [mockSignature, {commitment: 'finalized'}],
          result: new Promise(() => {}), // Never resolve.
        });
        clock.runAllAsync();

        const confirmationPromise =
          connection.confirmTransaction(mockSignature);
        clock.runAllAsync();

        // Return a signature status with a lower finality through the RPC API.
        await mockRpcResponse({
          method: 'getSignatureStatuses',
          params: [[mockSignature]],
          value: [
            {
              slot: 0,
              confirmations: null,
              confirmationStatus: 'processed', // Lower than we expect
              err: null,
            },
          ],
        });
        clock.runAllAsync();

        await expect(confirmationPromise).to.be.rejectedWith(
          TransactionExpiredTimeoutError,
        );
      });
    });
  }

  describe('transaction confirmation', () => {
    it('confirm transaction - error', async () => {
      const badTransactionSignature = 'bad transaction signature';

      await expect(
        connection.confirmTransaction({
          blockhash: 'sampleBlockhash',
          lastValidBlockHeight: 9999,
          signature: badTransactionSignature,
        }),
      ).to.be.rejectedWith('signature must be base58 encoded');

      await mockRpcResponse({
        method: 'getSignatureStatuses',
        params: [[badTransactionSignature]],
        error: mockErrorResponse,
      });

      await expect(
        connection.getSignatureStatus(badTransactionSignature),
      ).to.be.rejectedWith(mockErrorMessage);
    });
  });

  it('get transaction count', async () => {
    await mockRpcResponse({
      method: 'getTransactionCount',
      params: [],
      value: 1000000,
    });

    const count = await connection.getTransactionCount();
    expect(count).to.be.at.least(0);
  });

  it('get total supply', async () => {
    await mockRpcResponse({
      method: 'getSupply',
      params: [],
      value: {
        total: 1000000,
        circulating: 100000,
        nonCirculating: 900000,
        nonCirculatingAccounts: [],
      },
      withContext: true,
    });

    const count = await connection.getTotalSupply();
    expect(count).to.be.at.least(0);
  });

  it('get minimum balance for rent exemption', async () => {
    await mockRpcResponse({
      method: 'getMinimumBalanceForRentExemption',
      params: [512],
      value: 1000000,
    });

    const count = await connection.getMinimumBalanceForRentExemption(512);
    expect(count).to.be.at.least(0);
  });

  it('get confirmed signatures for address', async () => {
    const connection = new Connection(url);

    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [1],
      value: {
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let address: PublicKey | undefined;
    let expectedSignature: string | undefined;
    while (!address || !expectedSignature) {
      const block = await connection.getConfirmedBlock(slot);
      if (block.transactions.length > 0) {
        const {signature, publicKey} =
          block.transactions[0].transaction.signatures[0];
        if (signature) {
          address = publicKey;
          expectedSignature = bs58.encode(signature);
          break;
        }
      }
      slot++;
    }

    // getConfirmedSignaturesForAddress tests...
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 0,
    });
    const mockSignature =
      '5SHZ9NwpnS9zYnauN7pnuborKf39zGMr11XpMC59VvRSeDJNcnYLecmdxXCVuBFPNQLdCBBjyZiNCL4KoHKr3tvz';
    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [slot, {transactionDetails: 'signatures', rewards: false}],
      value: {
        blockTime: 1614281964,
        blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 1,
        signatures: [mockSignature],
      },
    });
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 123,
    });
    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [slot + 2, {transactionDetails: 'signatures', rewards: false}],
      value: {
        blockTime: 1614281964,
        blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
        parentSlot: 1,
        signatures: [mockSignature],
      },
    });
    await mockRpcResponse({
      method: 'getConfirmedSignaturesForAddress2',
      params: [address.toBase58(), {before: mockSignature}],
      value: [
        {
          signature: expectedSignature,
          slot,
          err: null,
          memo: null,
        },
      ],
    });

    const confirmedSignatures =
      await connection.getConfirmedSignaturesForAddress(
        address,
        slot,
        slot + 1,
      );
    expect(confirmedSignatures.includes(expectedSignature)).to.be.true;

    const badSlot = Number.MAX_SAFE_INTEGER - 1;
    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [badSlot - 1, {transactionDetails: 'signatures', rewards: false}],
      error: {message: 'Block not available for slot ' + badSlot},
    });
    expect(
      connection.getConfirmedSignaturesForAddress(
        address,
        badSlot,
        badSlot + 1,
      ),
    ).to.be.rejected;

    // getConfirmedSignaturesForAddress2 tests...
    await mockRpcResponse({
      method: 'getConfirmedSignaturesForAddress2',
      params: [address.toBase58(), {limit: 1}],
      value: [
        {
          signature: expectedSignature,
          slot,
          err: null,
          memo: null,
        },
      ],
    });

    const confirmedSignatures2 =
      await connection.getConfirmedSignaturesForAddress2(address, {limit: 1});
    expect(confirmedSignatures2).to.have.length(1);
    if (mockServer) {
      expect(confirmedSignatures2[0].signature).to.eq(expectedSignature);
      expect(confirmedSignatures2[0].slot).to.eq(slot);
      expect(confirmedSignatures2[0].err).to.be.null;
      expect(confirmedSignatures2[0].memo).to.be.null;
    }
  });

  it('get signatures for address', async () => {
    const connection = new Connection(url);

    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [1],
      value: {
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let address: PublicKey | undefined;
    let expectedSignature: string | undefined;
    while (!address || !expectedSignature) {
      const block = await connection.getConfirmedBlock(slot);
      if (block.transactions.length > 0) {
        const {signature, publicKey} =
          block.transactions[0].transaction.signatures[0];
        if (signature) {
          address = publicKey;
          expectedSignature = bs58.encode(signature);
          break;
        }
      }
      slot++;
    }

    // getSignaturesForAddress tests...
    await mockRpcResponse({
      method: 'getSignaturesForAddress',
      params: [address.toBase58(), {limit: 1}],
      value: [
        {
          signature: expectedSignature,
          slot,
          err: null,
          memo: null,
        },
      ],
    });

    const signatures = await connection.getSignaturesForAddress(address, {
      limit: 1,
    });
    expect(signatures).to.have.length(1);
    if (mockServer) {
      expect(signatures[0].signature).to.eq(expectedSignature);
      expect(signatures[0].slot).to.eq(slot);
      expect(signatures[0].err).to.be.null;
      expect(signatures[0].memo).to.be.null;
    }
  });

  it('get parsed confirmed transactions', async () => {
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [1],
      value: {
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let confirmedTransaction: string | undefined;
    while (!confirmedTransaction) {
      const block = await connection.getConfirmedBlock(slot);
      for (const tx of block.transactions) {
        if (tx.transaction.signature) {
          confirmedTransaction = bs58.encode(tx.transaction.signature);
          break;
        }
      }
      slot++;
    }

    await mockRpcBatchResponse({
      batch: [
        {
          methodName: 'getConfirmedTransaction',
          args: [],
        },
      ],
      result: [
        {
          blockTime: 1616102519,
          meta: {
            err: null,
            fee: 5000,
            innerInstructions: [],
            logMessages: [
              'Program Vote111111111111111111111111111111111111111 invoke [1]',
              'Program Vote111111111111111111111111111111111111111 success',
            ],
            postBalances: [499999995000, 26858640, 1, 1, 1],
            postTokenBalances: [],
            preBalances: [500000000000, 26858640, 1, 1, 1],
            preTokenBalances: [],
            status: {
              Ok: null,
            },
          },
          slot: 2,
          transaction: {
            message: {
              accountKeys: [
                {
                  pubkey: 'jcU4R7JccGEvDpe1i6bahvHpe47XahMXacG73EzE198',
                  signer: true,
                  writable: true,
                },
                {
                  pubkey: 'GfBcnCAU7kWfAYqKRCNyWEHjdEJZmzRZvEcX5bbzEQqt',
                  signer: false,
                  writable: true,
                },
                {
                  pubkey: 'SysvarS1otHashes111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
                {
                  pubkey: 'SysvarC1ock11111111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
                {
                  pubkey: 'Vote111111111111111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
              ],
              instructions: [
                {
                  parsed: {
                    info: {
                      clockSysvar:
                        'SysvarC1ock11111111111111111111111111111111',
                      slotHashesSysvar:
                        'SysvarS1otHashes111111111111111111111111111',
                      vote: {
                        hash: 'GuCya3AAGxn1qhoqxqy3WEdZdZUkXKpa9pthQ3tqvbpx',
                        slots: [1],
                        timestamp: 1616102669,
                      },
                      voteAccount:
                        'GfBcnCAU7kWfAYqKRCNyWEHjdEJZmzRZvEcX5bbzEQqt',
                      voteAuthority:
                        'jcU4R7JccGEvDpe1i6bahvHpe47XahMXacG73EzE198',
                    },
                    type: 'vote',
                  },
                  program: 'vote',
                  programId: 'Vote111111111111111111111111111111111111111',
                },
              ],
              recentBlockhash: 'G9ywjV5CVgMtLXruXtrE7af4QgFKYNXgDTw4jp7SWcSo',
            },
            signatures: [
              '4G4rTqnUdzrmBHsdKJSiMtonpQLWSw1avJ8YxWQ95jE6iFFHFsEkBnoYycxnkBS9xHWRc6EarDsrFG9USFBbjfjx',
            ],
          },
        },
        {
          blockTime: 1616102519,
          meta: {
            err: null,
            fee: 5000,
            innerInstructions: [],
            logMessages: [
              'Program Vote111111111111111111111111111111111111111 invoke [1]',
              'Program Vote111111111111111111111111111111111111111 success',
            ],
            postBalances: [499999995000, 26858640, 1, 1, 1],
            postTokenBalances: [],
            preBalances: [500000000000, 26858640, 1, 1, 1],
            preTokenBalances: [],
            status: {
              Ok: null,
            },
          },
          slot: 2,
          transaction: {
            message: {
              accountKeys: [
                {
                  pubkey: 'jcU4R7JccGEvDpe1i6bahvHpe47XahMXacG73EzE198',
                  signer: true,
                  writable: true,
                },
                {
                  pubkey: 'GfBcnCAU7kWfAYqKRCNyWEHjdEJZmzRZvEcX5bbzEQqt',
                  signer: false,
                  writable: true,
                },
                {
                  pubkey: 'SysvarS1otHashes111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
                {
                  pubkey: 'SysvarC1ock11111111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
                {
                  pubkey: 'Vote111111111111111111111111111111111111111',
                  signer: false,
                  writable: false,
                },
              ],
              instructions: [
                {
                  parsed: {
                    info: {
                      clockSysvar:
                        'SysvarC1ock11111111111111111111111111111111',
                      slotHashesSysvar:
                        'SysvarS1otHashes111111111111111111111111111',
                      vote: {
                        hash: 'GuCya3AAGxn1qhoqxqy3WEdZdZUkXKpa9pthQ3tqvbpx',
                        slots: [1],
                        timestamp: 1616102669,
                      },
                      voteAccount:
                        'GfBcnCAU7kWfAYqKRCNyWEHjdEJZmzRZvEcX5bbzEQqt',
                      voteAuthority:
                        'jcU4R7JccGEvDpe1i6bahvHpe47XahMXacG73EzE198',
                    },
                    type: 'vote',
                  },
                  program: 'vote',
                  programId: 'Vote111111111111111111111111111111111111111',
                },
              ],
              recentBlockhash: 'G9ywjV5CVgMtLXruXtrE7af4QgFKYNXgDTw4jp7SWcSo',
            },
            signatures: [
              '4G4rTqnUdzrmBHsdKJSiMtonpQLWSw1avJ8YxWQ95jE6iFFHFsEkBnoYycxnkBS9xHWRc6EarDsrFG9USFBbjfjx',
            ],
          },
        },
      ],
    });

    let result = await connection.getParsedConfirmedTransactions([
      confirmedTransaction,
      confirmedTransaction,
    ]);

    if (!result) {
      expect(result).to.be.ok;
      return;
    }

    expect(result).to.be.length(2);
    expect(result[0]).to.not.be.null;
    expect(result[1]).to.not.be.null;
    if (result[0] !== null) {
      expect(result[0].transaction.signatures).not.to.be.null;
    }
    if (result[1] !== null) {
      expect(result[1].transaction.signatures).not.to.be.null;
    }

    result = await connection.getParsedConfirmedTransactions([]);
    if (!result) {
      expect(result).to.be.ok;
      return;
    }

    expect(result).to.be.empty;
  });

  it('get block height', async () => {
    const commitment: Commitment = 'confirmed';

    await mockRpcResponse({
      method: 'getBlockHeight',
      params: [{commitment: commitment}],
      value: 10,
    });

    const blockHeight = await connection.getBlockHeight(commitment);
    expect(blockHeight).to.be.a('number');
  });

  it('get block production', async () => {
    const commitment: Commitment = 'processed';

    // Find slot of the lowest confirmed block
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let firstSlot = await connection.getFirstAvailableBlock();

    // Find current block height
    await mockRpcResponse({
      method: 'getBlockHeight',
      params: [{commitment: commitment}],
      value: 10,
    });
    let lastSlot = await connection.getBlockHeight(commitment);

    const blockProductionConfig = {
      commitment: commitment,
      range: {
        firstSlot,
        lastSlot,
      },
    };

    const blockProductionRet = {
      byIdentity: {
        '85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr': [12, 10],
      },
      range: {
        firstSlot,
        lastSlot,
      },
    };

    //mock RPC call with config specified
    await mockRpcResponse({
      method: 'getBlockProduction',
      params: [blockProductionConfig],
      value: blockProductionRet,
      withContext: true,
    });

    //mock RPC call with commitment only
    await mockRpcResponse({
      method: 'getBlockProduction',
      params: [{commitment: commitment}],
      value: blockProductionRet,
      withContext: true,
    });

    const result = await connection.getBlockProduction(blockProductionConfig);

    if (!result) {
      expect(result).to.be.ok;
      return;
    }

    expect(result.context).to.be.ok;
    expect(result.value).to.be.ok;

    const resultContextSlot = result.context.slot;
    expect(resultContextSlot).to.be.a('number');

    const resultIdentityDictionary = result.value.byIdentity;
    expect(resultIdentityDictionary).to.be.a('object');

    for (var key in resultIdentityDictionary) {
      expect(key).to.be.a('string');
      expect(resultIdentityDictionary[key]).to.be.a('array');
      expect(resultIdentityDictionary[key][0]).to.be.a('number');
      expect(resultIdentityDictionary[key][1]).to.be.a('number');
    }

    const resultSlotRange = result.value.range;
    expect(resultSlotRange.firstSlot).to.equal(firstSlot);
    expect(resultSlotRange.lastSlot).to.equal(lastSlot);

    const resultCommitmentOnly = await connection.getBlockProduction(
      commitment,
    );

    if (!resultCommitmentOnly) {
      expect(resultCommitmentOnly).to.be.ok;
      return;
    }
    expect(resultCommitmentOnly.context).to.be.ok;
    expect(resultCommitmentOnly.value).to.be.ok;

    const resultCOContextSlot = result.context.slot;
    expect(resultCOContextSlot).to.be.a('number');

    const resultCOIdentityDictionary = result.value.byIdentity;
    expect(resultCOIdentityDictionary).to.be.a('object');

    for (var property in resultCOIdentityDictionary) {
      expect(property).to.be.a('string');
      expect(resultCOIdentityDictionary[property]).to.be.a('array');
      expect(resultCOIdentityDictionary[property][0]).to.be.a('number');
      expect(resultCOIdentityDictionary[property][1]).to.be.a('number');
    }

    const resultCOSlotRange = result.value.range;
    expect(resultCOSlotRange.firstSlot).to.equal(firstSlot);
    expect(resultCOSlotRange.lastSlot).to.equal(lastSlot);
  });

  it('get transaction', async () => {
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getBlock',
      params: [1],
      value: {
        blockHeight: 0,
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let transaction: string | undefined;
    while (!transaction) {
      const block = await connection.getBlock(slot);
      if (block && block.transactions.length > 0) {
        transaction = block.transactions[0].transaction.signatures[0];
        continue;
      }
      slot++;
    }

    await mockRpcResponse({
      method: 'getTransaction',
      params: [transaction],
      value: {
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
                data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    const result = await connection.getTransaction(transaction);

    if (!result) {
      expect(result).to.be.ok;
      return;
    }

    const resultSignature = result.transaction.signatures[0];
    expect(resultSignature).to.eq(transaction);

    const newAddress = Keypair.generate().publicKey;
    const recentSignature = await helpers.airdrop({
      connection,
      address: newAddress,
      amount: 1,
    });

    await mockRpcResponse({
      method: 'getTransaction',
      params: [recentSignature],
      value: null,
    });

    // Signature hasn't been finalized yet
    const nullResponse = await connection.getTransaction(recentSignature);
    expect(nullResponse).to.be.null;
  });

  it('get confirmed transaction', async () => {
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getConfirmedBlock',
      params: [1],
      value: {
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let confirmedTransaction: string | undefined;
    while (!confirmedTransaction) {
      const block = await connection.getConfirmedBlock(slot);
      for (const tx of block.transactions) {
        if (tx.transaction.signature) {
          confirmedTransaction = bs58.encode(tx.transaction.signature);
          break;
        }
      }
      slot++;
    }

    await mockRpcResponse({
      method: 'getConfirmedTransaction',
      params: [confirmedTransaction],
      value: {
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
                data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    const result = await connection.getConfirmedTransaction(
      confirmedTransaction,
    );

    if (!result) {
      expect(result).to.be.ok;
      return;
    }

    if (result.transaction.signature === null) {
      expect(result.transaction.signature).not.to.be.null;
      return;
    }

    const resultSignature = bs58.encode(result.transaction.signature);
    expect(resultSignature).to.eq(confirmedTransaction);

    const newAddress = Keypair.generate().publicKey;
    const recentSignature = await helpers.airdrop({
      connection,
      address: newAddress,
      amount: 1,
    });

    await mockRpcResponse({
      method: 'getConfirmedTransaction',
      params: [recentSignature],
      value: null,
    });

    // Signature hasn't been finalized yet
    const nullResponse = await connection.getConfirmedTransaction(
      recentSignature,
    );
    expect(nullResponse).to.be.null;
  });

  it('get transactions', async function () {
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 1,
    });

    while ((await connection.getSlot()) <= 0) {
      continue;
    }

    await mockRpcResponse({
      method: 'getBlock',
      params: [1],
      value: {
        blockHeight: 0,
        blockTime: 1614281964,
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
                    data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
    });

    // Find a block that has a transaction.
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    let slot = await connection.getFirstAvailableBlock();

    let transaction: string | undefined;
    while (!transaction) {
      const block = await connection.getBlock(slot);
      if (block && block.transactions.length > 0) {
        transaction = block.transactions[0].transaction.signatures[0];
        continue;
      }
      slot++;
    }

    await mockRpcBatchResponse({
      batch: [
        {
          methodName: 'getTransaction',
          args: [transaction],
        },
      ],
      result: [
        {
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
                  data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
      ],
    });
    const [firstResult] = await connection.getTransactions([transaction]);
    if (firstResult == null) {
      expect.fail('Expected `getTransactions()` to return one result');
    }
    expect(firstResult.transaction.message.isAccountSigner(0)).to.be.true;
  });

  if (mockServer) {
    it('get parsed confirmed transaction coerces public keys of inner instructions', async () => {
      const confirmedTransaction: TransactionSignature =
        '4ADvAUQYxkh4qWKYE9QLW8gCLomGG94QchDLG4quvpBz1WqARYvzWQDDitKduAKspuy1DjcbnaDAnCAfnKpJYs48';

      function getMockData(inner: any) {
        return {
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
                  data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
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
        };
      }

      await mockRpcResponse({
        method: 'getConfirmedTransaction',
        params: [confirmedTransaction, {encoding: 'jsonParsed'}],
        value: getMockData({
          parsed: {},
          program: 'spl-token',
          programId: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
        }),
      });

      const result = await connection.getParsedConfirmedTransaction(
        confirmedTransaction,
      );

      if (result && result.meta && result.meta.innerInstructions) {
        const innerInstructions = result.meta.innerInstructions;
        const firstIx = innerInstructions[0].instructions[0];
        expect(firstIx.programId).to.be.instanceOf(PublicKey);
      }

      await mockRpcResponse({
        method: 'getConfirmedTransaction',
        params: [confirmedTransaction, {encoding: 'jsonParsed'}],
        value: getMockData({
          accounts: [
            'EeJqWk5pczNjsqqY3jia9xfFNG1dD68te4s8gsdCuEk7',
            '6tVrjJhFm5SAvvdh6tysjotQurCSELpxuW3JaAAYeC1m',
          ],
          data: 'ai3535',
          programId: 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
        }),
      });

      const result2 = await connection.getParsedConfirmedTransaction(
        confirmedTransaction,
      );

      if (result2 && result2.meta && result2.meta.innerInstructions) {
        const innerInstructions = result2.meta.innerInstructions;
        const instruction = innerInstructions[0].instructions[0];
        expect(instruction.programId).to.be.instanceOf(PublicKey);
        if ('accounts' in instruction) {
          expect(instruction.accounts[0]).to.be.instanceOf(PublicKey);
          expect(instruction.accounts[1]).to.be.instanceOf(PublicKey);
        } else {
          expect('accounts' in instruction).to.be.true;
        }
      }
    });
  }

  describe('get parsed block', function () {
    it('can deserialize a response when `transactionDetails` is `full`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "full"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            encoding: 'jsonParsed',
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'full',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
          transactions: [
            {
              meta: {
                err: null,
                fee: 5000,
                innerInstructions: [],
                logMessages: [
                  'Program Vote111111111111111111111111111111111111111 invoke [1]',
                  'Program Vote111111111111111111111111111111111111111 success',
                ],
                postBalances: [3712706991, 5765419239, 1169280, 143487360, 1],
                postTokenBalances: [],
                preBalances: [3712711991, 5765419239, 1169280, 143487360, 1],
                preTokenBalances: [],
                rewards: null,
                status: {Ok: null},
              },
              transaction: {
                message: {
                  accountKeys: [
                    {
                      pubkey: '7v5fMKBqC9PuwjSdS9k9JU7efEXmq3bHTMF5fuSHnqrm',
                      signer: true,
                      source: 'transaction',
                      writable: true,
                    },
                    {
                      pubkey: 'AhcvnNdppGEcgdpK5gfcaZnAWz4ct8V4n7De5QiLiuzG',
                      signer: false,
                      source: 'transaction',
                      writable: true,
                    },
                    {
                      pubkey: 'SysvarC1ock11111111111111111111111111111111',
                      signer: false,
                      source: 'transaction',
                      writable: false,
                    },
                    {
                      pubkey: 'SysvarS1otHashes111111111111111111111111111',
                      signer: false,
                      source: 'transaction',
                      writable: false,
                    },
                    {
                      pubkey: 'Vote111111111111111111111111111111111111111',
                      signer: false,
                      source: 'transaction',
                      writable: false,
                    },
                  ],
                  addressTableLookups: null,
                  instructions: [
                    {
                      parsed: {
                        info: {
                          clockSysvar:
                            'SysvarC1ock11111111111111111111111111111111',
                          slotHashesSysvar:
                            'SysvarS1otHashes111111111111111111111111111',
                          vote: {
                            hash: '2gmQ8xMjZaXn63kr8qzPAUjQAHi7xCDjSibPdJxhVYMm',
                            slots: [164153060, 164153061],
                            timestamp: 1669845645,
                          },
                          voteAccount:
                            'AhcvnNdppGEcgdpK5gfcaZnAWz4ct8V4n7De5QiLiuzG',
                          voteAuthority:
                            '7v5fMKBqC9PuwjSdS9k9JU7efEXmq3bHTMF5fuSHnqrm',
                        },
                        type: 'vote',
                      },
                      program: 'vote',
                      programId: 'Vote111111111111111111111111111111111111111',
                    },
                  ],
                  recentBlockhash:
                    'GLqYrN6AQxCGtFTQywkPj2WN5tafC3KerBhW4QkmAyD4',
                },
                signatures: [
                  '5qDZ3nUUwp8VHFfAE5ydTQRULCoVLMGs16EprwdXsvyNCLe1NfckCkRE4BPi6wyEW9hXvG9iWU2prXfbM8SNPVEC',
                ],
              },
              version: 'legacy',
            },
          ],
        },
      });
      await expect(
        connection.getParsedBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'full',
        }),
      ).not.to.eventually.be.rejected;
    });

    it('can deserialize a response when `transactionDetails` is `none`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "none"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            encoding: 'jsonParsed',
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'none',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
        },
      });
      await expect(
        connection.getParsedBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'none',
        }),
      ).not.to.eventually.be.rejected;
    });

    it('can deserialize a response when `transactionDetails` is `accounts`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "accounts"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            encoding: 'jsonParsed',
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'accounts',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
          transactions: [
            {
              meta: {
                err: null,
                fee: 5000,
                postBalances: [18237691394, 26858640, 1169280, 143487360, 1],
                postTokenBalances: [],
                preBalances: [18237696394, 26858640, 1169280, 143487360, 1],
                preTokenBalances: [],
                status: {Ok: null},
              },
              transaction: {
                accountKeys: [
                  {
                    pubkey: '914RFshndUeZaNPjf8UWDCyo49ahQ1XQ2w9BnEMwpHKF',
                    signer: true,
                    source: 'transaction',
                    writable: true,
                  },
                  {
                    pubkey: '4cCd4SGrMswhqboYBJ5AcCVvCjh5NtaeZNwWFJzsnUWY',
                    signer: false,
                    source: 'transaction',
                    writable: true,
                  },
                  {
                    pubkey: 'SysvarC1ock11111111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                  {
                    pubkey: 'SysvarS1otHashes111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                  {
                    pubkey: 'Vote111111111111111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                ],
                signatures: [
                  '5ZDp1HfNZhNRHc75ncsiZ4sCq1fGJHMGf9u36M3foD5PMH4Xu5S4X2x7aryn4JinUdG11oSYCk7zxbNmLJzzqUft',
                ],
              },
              version: 'legacy',
            },
          ],
        },
      });
      await expect(
        connection.getParsedBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'accounts',
        }),
      ).not.to.eventually.be.rejected;
    });
  });

  describe('get block', function () {
    beforeEach(async function () {
      await mockRpcResponse({
        method: 'getSlot',
        params: [],
        value: 1,
      });

      while ((await connection.getSlot()) <= 0) {
        continue;
      }
    });

    it('gets the genesis block', async function () {
      await mockRpcResponse({
        method: 'getBlock',
        params: [0],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          transactions: [],
        },
      });

      let maybeBlock0: BlockResponse | null;
      try {
        maybeBlock0 = await connection.getBlock(0);
      } catch (e) {
        if (process.env.TEST_LIVE) {
          console.warn(
            'WARNING: We ran no assertions about the genesis block because block 0 ' +
              'could not be found. See https://github.com/solana-labs/solana/issues/23853.',
          );
          this.skip();
        } else {
          throw e;
        }
      }
      expect(maybeBlock0).not.to.be.null;
      const block0 = maybeBlock0!;

      // Block 0 never has any transactions in test validator
      const blockhash0 = block0.blockhash;
      expect(block0.transactions).to.have.length(0);
      expect(blockhash0).not.to.be.null;
      expect(block0.previousBlockhash).not.to.be.null;
      expect(block0.parentSlot).to.eq(0);
    });

    it('gets a block having a parent', async function () {
      // Mock parent of block with transaction.
      await mockRpcResponse({
        method: 'getBlock',
        params: [0],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          transactions: [],
        },
      });
      // Mock block with transaction.
      await mockRpcResponse({
        method: 'getBlock',
        params: [1],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
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
                      data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                      programIdIndex: 4,
                    },
                  ],
                  recentBlockhash:
                    'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
                },
                signatures: [
                  'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
                  '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
                ],
              },
            },
          ],
        },
      });

      // Find a block that has a transaction *and* a parent.
      await mockRpcResponse({
        method: 'getFirstAvailableBlock',
        params: [],
        value: 0,
      });
      let candidateSlot = (await connection.getFirstAvailableBlock()) + 1;
      let result:
        | {
            blockWithTransaction: BlockResponse;
            parentBlock: BlockResponse;
          }
        | undefined;
      while (!result) {
        const candidateBlock = await connection.getBlock(candidateSlot);
        if (candidateBlock && candidateBlock.transactions.length) {
          const parentBlock = await connection.getBlock(candidateSlot - 1);
          if (parentBlock) {
            result = {blockWithTransaction: candidateBlock, parentBlock};
            break;
          }
        }
        candidateSlot++;
      }

      // Compare data with parent
      expect(result.blockWithTransaction.previousBlockhash).to.eq(
        result.parentBlock.blockhash,
      );
      expect(result.blockWithTransaction.blockhash).not.to.be.null;
      expect(result.blockWithTransaction.transactions[0].transaction).not.to.be
        .null;

      await mockRpcResponse({
        method: 'getBlock',
        params: [Number.MAX_SAFE_INTEGER],
        error: {
          message: `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
        },
      });
      await expect(
        connection.getBlock(Number.MAX_SAFE_INTEGER),
      ).to.be.rejectedWith(
        `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
      );
    });

    it('can deserialize a response when `transactionDetails` is `full`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "full"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'full',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
          transactions: [
            {
              meta: {
                err: null,
                fee: 5000,
                innerInstructions: [],
                loadedAddresses: {readonly: [], writable: []},
                logMessages: [
                  'Program Vote111111111111111111111111111111111111111 invoke [1]',
                  'Program Vote111111111111111111111111111111111111111 success',
                ],
                postBalances: [12278161908, 39995373, 1169280, 143487360, 1],
                postTokenBalances: [],
                preBalances: [12278166908, 39995373, 1169280, 143487360, 1],
                preTokenBalances: [],
                rewards: null,
                status: {Ok: null},
              },
              transaction: {
                message: {
                  accountKeys: [
                    'FTWuJ2tqjecNizCSE66z4BD1tBHomG6DVffGUwRuWUkM',
                    'H2z3pBT62ByS4jpqsiEMtgN3NUFEuZHiTvoKCFjqCtD6',
                    'SysvarC1ock11111111111111111111111111111111',
                    'SysvarS1otHashes111111111111111111111111111',
                    'Vote111111111111111111111111111111111111111',
                  ],
                  header: {
                    numReadonlySignedAccounts: 0,
                    numReadonlyUnsignedAccounts: 3,
                    numRequiredSignatures: 1,
                  },
                  instructions: [
                    {
                      accounts: [1, 3, 2, 0],
                      data: '29z5mr1JoRmJYQ6zG7p2F3mu68pWTNw9q49Tu7KrSEgoS6Jh1LMPGUK3HXs1N3Dody3icCcXxu6xPYoXLWnUTafEGm3knK',
                      programIdIndex: 4,
                    },
                  ],
                  recentBlockhash:
                    'GLqYrN6AQxCGtFTQywkPj2WN5tafC3KerBhW4QkmAyD4',
                },
                signatures: [
                  '4SZofEnXEVzCYvzk16z6ScR6F3iNtZ3FsCC1PEWegpzvGwTJR6x9cDi8VHRmCFGC5XFs2yEFms3j36Mj7XVyHXbb',
                ],
              },
              version: 'legacy',
            },
          ],
        },
      });
      await expect(
        connection.getBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'full',
        }),
      ).not.to.eventually.be.rejected;
    });

    it('can deserialize a response when `transactionDetails` is `none`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "none"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'none',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
        },
      });
      await expect(
        connection.getBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'none',
        }),
      ).not.to.eventually.be.rejected;
    });

    it('can deserialize a response when `transactionDetails` is `accounts`', async () => {
      // Mock block with transaction, fetched using `"transactionDetails": "accounts"`.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            maxSupportedTransactionVersion: 0,
            transactionDetails: 'accounts',
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: '49d2UbduiZWjtR3Wvfv2t2QxmXvtZNWSPFRZxEDYAvQN',
          parentSlot: 0,
          previousBlockhash: 'mDd5yMLfuroS1JVZMHo2VZLTgKXXNBXrzPR5UkzFD4X',
          transactions: [
            {
              meta: {
                err: null,
                fee: 5000,
                postBalances: [2751549948, 11751747405, 1169280, 143487360, 1],
                postTokenBalances: [],
                preBalances: [2751554948, 11751747405, 1169280, 143487360, 1],
                preTokenBalances: [],
                status: {Ok: null},
              },
              transaction: {
                accountKeys: [
                  {
                    pubkey: 'D7hwgGRTr1vaCxzmfEKCaf56SPgBJmjHh6UXHG3p12bB',
                    signer: true,
                    source: 'transaction',
                    writable: true,
                  },
                  {
                    pubkey: '8iLE53Y9k4sccy4gxrT936BHbhYS6J13kQT5vRXhXFMX',
                    signer: false,
                    source: 'transaction',
                    writable: true,
                  },
                  {
                    pubkey: 'SysvarC1ock11111111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                  {
                    pubkey: 'SysvarS1otHashes111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                  {
                    pubkey: 'Vote111111111111111111111111111111111111111',
                    signer: false,
                    source: 'transaction',
                    writable: false,
                  },
                ],
                signatures: [
                  'uNKj2ogn8ZRRjyVWXLC7sLRWpKQyMUomm66RXoDuWLXikPSJN8C7ZZK95j8S2bzcjwH6MvrXKSHtCWEURPpEXMB',
                ],
              },
              version: 'legacy',
            },
          ],
        },
      });
      await expect(
        connection.getBlock(1, {
          maxSupportedTransactionVersion: 0,
          transactionDetails: 'accounts',
        }),
      ).not.to.eventually.be.rejected;
    });
  });

  describe('get confirmed block', function () {
    beforeEach(async function () {
      await mockRpcResponse({
        method: 'getSlot',
        params: [],
        value: 1,
      });

      while ((await connection.getSlot()) <= 0) {
        continue;
      }
    });

    it('gets the genesis block', async function () {
      await mockRpcResponse({
        method: 'getConfirmedBlock',
        params: [0],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          transactions: [],
        },
      });

      let block0: ConfirmedBlock;
      try {
        block0 = await connection.getConfirmedBlock(0);
      } catch (e) {
        if (process.env.TEST_LIVE) {
          console.warn(
            'WARNING: We ran no assertions about the genesis block because block 0 ' +
              'could not be found. See https://github.com/solana-labs/solana/issues/23853.',
          );
          this.skip();
        } else {
          throw e;
        }
      }

      // Block 0 never has any transactions in test validator
      const blockhash0 = block0.blockhash;
      expect(block0.transactions).to.have.length(0);
      expect(blockhash0).not.to.be.null;
      expect(block0.previousBlockhash).not.to.be.null;
      expect(block0.parentSlot).to.eq(0);
    });

    it('gets a block having a parent', async function () {
      // Mock parent of block with transaction.
      await mockRpcResponse({
        method: 'getConfirmedBlock',
        params: [0],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          transactions: [],
        },
      });
      // Mock block with transaction.
      await mockRpcResponse({
        method: 'getConfirmedBlock',
        params: [1],
        value: {
          blockTime: 1614281964,
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
                      data: '37u9WtQpcm6ULa3VtWDFAWoQc1hUvybPrA3dtx99tgHvvcE7pKRZjuGmn7VX2tC3JmYDYGG7',
                      programIdIndex: 4,
                    },
                  ],
                  recentBlockhash:
                    'GeyAFFRY3WGpmam2hbgrKw4rbU2RKzfVLm5QLSeZwTZE',
                },
                signatures: [
                  'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
                  '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
                ],
              },
            },
          ],
        },
      });

      // Find a block that has a transaction *and* a parent.
      await mockRpcResponse({
        method: 'getFirstAvailableBlock',
        params: [],
        value: 0,
      });
      let candidateSlot = (await connection.getFirstAvailableBlock()) + 1;
      let result:
        | {
            blockWithTransaction: ConfirmedBlock;
            parentBlock: ConfirmedBlock;
          }
        | undefined;
      while (!result) {
        const candidateBlock = await connection.getConfirmedBlock(
          candidateSlot,
        );
        if (candidateBlock && candidateBlock.transactions.length) {
          const parentBlock = await connection.getConfirmedBlock(
            candidateSlot - 1,
          );
          if (parentBlock) {
            result = {blockWithTransaction: candidateBlock, parentBlock};
            break;
          }
        }
        candidateSlot++;
      }

      // Compare data with parent
      expect(result.blockWithTransaction.previousBlockhash).to.eq(
        result.parentBlock.blockhash,
      );
      expect(result.blockWithTransaction.blockhash).not.to.be.null;
      expect(result.blockWithTransaction.transactions[0].transaction).not.to.be
        .null;

      await mockRpcResponse({
        method: 'getConfirmedBlock',
        params: [Number.MAX_SAFE_INTEGER],
        error: {
          message: `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
        },
      });
      await expect(
        connection.getConfirmedBlock(Number.MAX_SAFE_INTEGER),
      ).to.be.rejectedWith(
        `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
      );
    });
  });

  it('get blocks between two slots', async () => {
    await mockRpcResponse({
      method: 'getBlocks',
      params: [0, 9],
      value: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    });
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 0,
    });
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 9,
    });

    while ((await connection.getSlot()) <= 1) {
      continue;
    }

    const [startSlot, latestSlot] = await Promise.all([
      connection.getFirstAvailableBlock(),
      connection.getSlot(),
    ]);
    const blocks = await connection.getBlocks(startSlot, latestSlot);
    expect(blocks).to.have.length(latestSlot - startSlot + 1);
    expect(blocks[0]).to.eq(startSlot);
    expect(blocks).to.contain(latestSlot);
  }).timeout(20 * 1000);

  it('get blocks from starting slot', async () => {
    await mockRpcResponse({
      method: 'getBlocks',
      params: [0],
      value: [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
        20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37,
        38, 39, 40, 41, 42,
      ],
    });
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 0,
    });
    await mockRpcResponse({
      method: 'getSlot',
      params: [],
      value: 20,
    });

    while ((await connection.getSlot()) <= 1) {
      continue;
    }

    const startSlot = await connection.getFirstAvailableBlock();
    const [blocks, latestSlot] = await Promise.all([
      connection.getBlocks(startSlot),
      connection.getSlot(),
    ]);
    if (mockServer) {
      expect(blocks).to.have.length(43);
    } else {
      expect(blocks).to.have.length(latestSlot - startSlot + 1);
    }
    expect(blocks[0]).to.eq(startSlot);
    expect(blocks).to.contain(latestSlot);
  }).timeout(20 * 1000);

  describe('get block signatures', function () {
    beforeEach(async function () {
      await mockRpcResponse({
        method: 'getSlot',
        params: [],
        value: 1,
      });

      while ((await connection.getSlot()) <= 0) {
        continue;
      }
    });

    it('gets the genesis block', async function () {
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          0,
          {
            transactionDetails: 'signatures',
            rewards: false,
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          signatures: [],
        },
      });

      let block0: BlockSignatures;
      try {
        block0 = await connection.getBlockSignatures(0);
      } catch (e) {
        if (process.env.TEST_LIVE) {
          console.warn(
            'WARNING: We ran no assertions about the genesis block because block 0 ' +
              'could not be found. See https://github.com/solana-labs/solana/issues/23853.',
          );
          this.skip();
        } else {
          throw e;
        }
      }

      // Block 0 never has any transactions in test validator
      const blockhash0 = block0.blockhash;
      expect(block0.signatures).to.have.length(0);
      expect(blockhash0).not.to.be.null;
      expect(block0.previousBlockhash).not.to.be.null;
      expect(block0.parentSlot).to.eq(0);
      expect(block0).to.not.have.property('rewards');
    });

    it('gets a block having a parent', async function () {
      // Mock parent of block with transaction.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          0,
          {
            transactionDetails: 'signatures',
            rewards: false,
          },
        ],
        value: {
          blockHeight: 0,
          blockTime: 1614281964,
          blockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          signatures: [],
        },
      });
      // Mock block with transaction.
      await mockRpcResponse({
        method: 'getBlock',
        params: [
          1,
          {
            transactionDetails: 'signatures',
            rewards: false,
          },
        ],
        value: {
          blockHeight: 1,
          blockTime: 1614281964,
          blockhash: '57zQNBZBEiHsCZFqsaY6h176ioXy5MsSLmcvHkEyaLGy',
          previousBlockhash: 'H5nJ91eGag3B5ZSRHZ7zG5ZwXJ6ywCt2hyR8xCsV7xMo',
          parentSlot: 0,
          signatures: [
            'w2Zeq8YkpyB463DttvfzARD7k9ZxGEwbsEw4boEK7jDp3pfoxZbTdLFSsEPhzXhpCcjGi2kHtHFobgX49MMhbWt',
            '4oCEqwGrMdBeMxpzuWiukCYqSfV4DsSKXSiVVCh1iJ6pS772X7y219JZP3mgqBz5PhsvprpKyhzChjYc3VSBQXzG',
          ],
        },
      });

      // Find a block that has a transaction *and* a parent.
      await mockRpcResponse({
        method: 'getFirstAvailableBlock',
        params: [],
        value: 0,
      });
      let candidateSlot = (await connection.getFirstAvailableBlock()) + 1;
      let result:
        | {
            blockWithTransaction: BlockSignatures;
            parentBlock: BlockSignatures;
          }
        | undefined;
      while (!result) {
        const candidateBlock = await connection.getBlockSignatures(
          candidateSlot,
        );
        if (candidateBlock && candidateBlock.signatures.length) {
          const parentBlock = await connection.getBlockSignatures(
            candidateSlot - 1,
          );
          if (parentBlock) {
            result = {blockWithTransaction: candidateBlock, parentBlock};
            break;
          }
        }
        candidateSlot++;
      }

      // Compare data with parent
      expect(result.blockWithTransaction.previousBlockhash).to.eq(
        result.parentBlock.blockhash,
      );
      expect(result.blockWithTransaction.blockhash).not.to.be.null;
      expect(result.blockWithTransaction.signatures[0]).not.to.be.null;
      expect(result.blockWithTransaction).to.not.have.property('rewards');

      await mockRpcResponse({
        method: 'getBlock',
        params: [Number.MAX_SAFE_INTEGER],
        error: {
          message: `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
        },
      });
      await expect(
        connection.getBlockSignatures(Number.MAX_SAFE_INTEGER),
      ).to.be.rejectedWith(
        `Block not available for slot ${Number.MAX_SAFE_INTEGER}`,
      );
    });
  });

  it('get recent blockhash', async () => {
    const commitments: Commitment[] = ['processed', 'confirmed', 'finalized'];
    for (const commitment of commitments) {
      const {blockhash, feeCalculator} = await helpers.recentBlockhash({
        connection,
        commitment,
      });
      expect(bs58.decode(blockhash)).to.have.length(32);
      expect(feeCalculator.lamportsPerSignature).to.be.at.least(0);
    }
  });

  it('get latest blockhash', async () => {
    const commitments: Commitment[] = ['processed', 'confirmed', 'finalized'];
    for (const commitment of commitments) {
      const {blockhash, lastValidBlockHeight} = await helpers.latestBlockhash({
        connection,
        commitment,
      });
      expect(bs58.decode(blockhash)).to.have.length(32);
      expect(lastValidBlockHeight).to.be.at.least(0);
    }
  });

  it('get fee calculator', async () => {
    const {blockhash} = await helpers.recentBlockhash({connection});
    await mockRpcResponse({
      method: 'getFeeCalculatorForBlockhash',
      params: [blockhash, {commitment: 'confirmed'}],
      value: {
        feeCalculator: {
          lamportsPerSignature: 5000,
        },
      },
      withContext: true,
    });

    const feeCalculator = (
      await connection.getFeeCalculatorForBlockhash(blockhash, 'confirmed')
    ).value;
    if (feeCalculator === null) {
      expect(feeCalculator).not.to.be.null;
      return;
    }
    expect(feeCalculator.lamportsPerSignature).to.eq(5000);
  });

  it('get fee for message (legacy)', async () => {
    const accountFrom = Keypair.generate();
    const accountTo = Keypair.generate();

    const latestBlockhash = await helpers.latestBlockhash({connection});

    const transaction = new Transaction({
      feePayer: accountFrom.publicKey,
      ...latestBlockhash,
    }).add(
      SystemProgram.transfer({
        fromPubkey: accountFrom.publicKey,
        toPubkey: accountTo.publicKey,
        lamports: 10,
      }),
    );
    const message = transaction.compileMessage();

    await mockRpcResponse({
      method: 'getFeeForMessage',
      params: [
        message.serialize().toString('base64'),
        {commitment: 'confirmed'},
      ],
      value: 5000,
      withContext: true,
    });

    const fee = (await connection.getFeeForMessage(message, 'confirmed')).value;
    expect(fee).to.eq(5000);
  });

  it('get fee for message (v0)', async () => {
    const accountFrom = Keypair.generate();
    const accountTo = Keypair.generate();

    const recentBlockhash = (await helpers.latestBlockhash({connection}))
      .blockhash;
    const instructions = [
      SystemProgram.transfer({
        fromPubkey: accountFrom.publicKey,
        toPubkey: accountTo.publicKey,
        lamports: 10,
      }),
    ];

    const messageV0 = new TransactionMessage({
      payerKey: accountFrom.publicKey,
      recentBlockhash,
      instructions,
    }).compileToV0Message();

    await mockRpcResponse({
      method: 'getFeeForMessage',
      params: [
        toBuffer(messageV0.serialize()).toString('base64'),
        {commitment: 'confirmed'},
      ],
      value: 5000,
      withContext: true,
    });

    const fee = (await connection.getFeeForMessage(messageV0, 'confirmed'))
      .value;
    expect(fee).to.eq(5000);
  });

  it('get block time', async () => {
    await mockRpcResponse({
      method: 'getBlockTime',
      params: [1],
      value: 10000,
    });

    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 1,
    });
    const slot = await connection.getFirstAvailableBlock();
    const blockTime = await connection.getBlockTime(slot);
    if (blockTime === null) {
      expect(blockTime).not.to.be.null;
    } else {
      expect(blockTime).to.be.greaterThan(0);
    }
  });

  it('get minimum ledger slot', async () => {
    await mockRpcResponse({
      method: 'minimumLedgerSlot',
      params: [],
      value: 0,
    });

    const minimumLedgerSlot = await connection.getMinimumLedgerSlot();
    expect(minimumLedgerSlot).to.be.at.least(0);
  });

  it('get first available block', async () => {
    await mockRpcResponse({
      method: 'getFirstAvailableBlock',
      params: [],
      value: 0,
    });

    const firstAvailableBlock = await connection.getFirstAvailableBlock();
    expect(firstAvailableBlock).to.be.at.least(0);
  });

  it('get supply', async () => {
    await mockRpcResponse({
      method: 'getSupply',
      params: [{commitment: 'finalized'}],
      value: {
        total: 1000,
        circulating: 100,
        nonCirculating: 900,
        nonCirculatingAccounts: [Keypair.generate().publicKey.toBase58()],
      },
      withContext: true,
    });

    const supply = (await connection.getSupply('finalized')).value;
    expect(supply.total).to.be.greaterThan(0);
    expect(supply.circulating).to.be.greaterThan(0);
    expect(supply.nonCirculating).to.be.at.least(0);
    expect(supply.nonCirculatingAccounts.length).to.be.at.least(0);
  });

  it('get supply without accounts', async () => {
    await mockRpcResponse({
      method: 'getSupply',
      params: [{commitment: 'finalized'}],
      value: {
        total: 1000,
        circulating: 100,
        nonCirculating: 900,
        nonCirculatingAccounts: [],
      },
      withContext: true,
    });

    const supply = (
      await connection.getSupply({
        commitment: 'finalized',
        excludeNonCirculatingAccountsList: true,
      })
    ).value;
    expect(supply.total).to.be.greaterThan(0);
    expect(supply.circulating).to.be.greaterThan(0);
    expect(supply.nonCirculating).to.be.at.least(0);
    expect(supply.nonCirculatingAccounts.length).to.eq(0);
  });

  [undefined, 'confirmed' as Commitment].forEach(function (commitment) {
    describe(
      "when the connection's default commitment is `" + commitment + '`',
      () => {
        let connectionWithCommitment: Connection;
        beforeEach(() => {
          connectionWithCommitment = new Connection(url, commitment);
        });
        it('get performance samples', async () => {
          await mockRpcResponse({
            method: 'getRecentPerformanceSamples',
            params: [],
            value: [
              {
                slot: 1234,
                numTransactions: 1000,
                numSlots: 60,
                samplePeriodSecs: 60,
              },
            ],
          });

          const perfSamples =
            await connectionWithCommitment.getRecentPerformanceSamples();
          expect(Array.isArray(perfSamples)).to.be.true;

          if (perfSamples.length > 0) {
            expect(perfSamples[0].slot).to.be.greaterThan(0);
            expect(perfSamples[0].numTransactions).to.be.greaterThan(0);
            expect(perfSamples[0].numSlots).to.be.greaterThan(0);
            expect(perfSamples[0].samplePeriodSecs).to.be.greaterThan(0);
          }
        });
      },
    );
  });

  it('get performance samples limit too high', async () => {
    await mockRpcResponse({
      method: 'getRecentPerformanceSamples',
      params: [100000],
      error: mockErrorResponse,
    });

    await expect(connection.getRecentPerformanceSamples(100000)).to.be.rejected;
  });

  const TOKEN_PROGRAM_ID = new PublicKey(
    'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',
  );

  if (process.env.TEST_LIVE) {
    describe('token methods', () => {
      const connection = new Connection(url, 'confirmed');
      const newAccount = PublicKey.unique();

      let payerKeypair = new Keypair();
      let testTokenMintPubkey: PublicKey;
      let testOwnerKeypair: Keypair;
      let testTokenAccountPubkey: PublicKey;
      let testSignature: TransactionSignature;

      // Setup token mints and accounts for token tests
      before(async function () {
        this.timeout(30 * 1000);

        await connection.confirmTransaction(
          await connection.requestAirdrop(payerKeypair.publicKey, 100000000000),
        );

        const mintOwnerKeypair = new Keypair();
        const accountOwnerKeypair = new Keypair();
        const mintPubkey = await splToken.createMint(
          connection as any,
          payerKeypair,
          mintOwnerKeypair.publicKey,
          null, // freeze authority
          2, // decimals
        );

        const tokenAccountPubkey = await splToken.createAccount(
          connection as any,
          payerKeypair,
          mintPubkey,
          accountOwnerKeypair.publicKey,
        );

        await splToken.mintTo(
          connection as any,
          payerKeypair,
          mintPubkey,
          tokenAccountPubkey,
          mintOwnerKeypair,
          11111,
        );

        const mintPubkey2 = await splToken.createMint(
          connection as any,
          payerKeypair,
          mintOwnerKeypair.publicKey,
          null, // freeze authority
          2, // decimals
        );

        const tokenAccountPubkey2 = await splToken.createAccount(
          connection as any,
          payerKeypair,
          mintPubkey2,
          accountOwnerKeypair.publicKey,
        );

        await splToken.mintTo(
          connection as any,
          payerKeypair,
          mintPubkey2,
          tokenAccountPubkey2,
          mintOwnerKeypair,
          100,
        );

        const tokenAccountDestPubkey = await splToken.createAccount(
          connection as any,
          payerKeypair,
          mintPubkey,
          accountOwnerKeypair.publicKey,
          new Keypair() as any,
        );

        testSignature = await splToken.transfer(
          connection as any,
          payerKeypair,
          tokenAccountPubkey,
          tokenAccountDestPubkey,
          accountOwnerKeypair,
          1,
        );

        testTokenMintPubkey = mintPubkey as PublicKey;
        testOwnerKeypair = accountOwnerKeypair;
        testTokenAccountPubkey = tokenAccountPubkey as PublicKey;
      });

      it('get token supply', async () => {
        const supply = (await connection.getTokenSupply(testTokenMintPubkey))
          .value;
        expect(supply.uiAmount).to.eq(111.11);
        expect(supply.decimals).to.eq(2);
        expect(supply.amount).to.eq('11111');

        await expect(connection.getTokenSupply(newAccount)).to.be.rejected;
      });

      it('get token largest accounts', async () => {
        const largestAccounts = (
          await connection.getTokenLargestAccounts(testTokenMintPubkey)
        ).value;

        expect(largestAccounts).to.have.length(2);
        const largestAccount = largestAccounts[0];
        expect(largestAccount.address.equals(testTokenAccountPubkey)).to.be
          .true;
        expect(largestAccount.amount).to.eq('11110');
        expect(largestAccount.decimals).to.eq(2);
        expect(largestAccount.uiAmount).to.eq(111.1);

        await expect(connection.getTokenLargestAccounts(newAccount)).to.be
          .rejected;
      });

      it('get confirmed token transaction', async () => {
        const parsedTx = await connection.getParsedConfirmedTransaction(
          testSignature,
        );
        if (parsedTx === null) {
          expect(parsedTx).not.to.be.null;
          return;
        }
        const {signatures, message} = parsedTx.transaction;
        expect(signatures[0]).to.eq(testSignature);
        const ix = message.instructions[0];
        if ('parsed' in ix) {
          expect(ix.program).to.eq('spl-token');
          expect(ix.programId).to.eql(TOKEN_PROGRAM_ID);
        } else {
          expect('parsed' in ix).to.be.true;
        }

        const missingSignature =
          '45pGoC4Rr3fJ1TKrsiRkhHRbdUeX7633XAGVec6XzVdpRbzQgHhe6ZC6Uq164MPWtiqMg7wCkC6Wy3jy2BqsDEKf';
        const nullResponse = await connection.getParsedConfirmedTransaction(
          missingSignature,
        );

        expect(nullResponse).to.be.null;
      });

      it('get token account balance', async () => {
        const balance = (
          await connection.getTokenAccountBalance(testTokenAccountPubkey)
        ).value;
        expect(balance.amount).to.eq('11110');
        expect(balance.decimals).to.eq(2);
        expect(balance.uiAmount).to.eq(111.1);

        await expect(connection.getTokenAccountBalance(newAccount)).to.be
          .rejected;
      });

      it('get parsed token account info', async () => {
        const accountInfo = (
          await connection.getParsedAccountInfo(testTokenAccountPubkey)
        ).value;
        if (accountInfo) {
          const data = accountInfo.data;
          if (Buffer.isBuffer(data)) {
            expect(Buffer.isBuffer(data)).to.eq(false);
          } else {
            expect(data.program).to.eq('spl-token');
            expect(data.parsed).to.be.ok;
          }
        }
      });

      it('get multiple parsed token accounts', async () => {
        const accounts = (
          await connection.getMultipleParsedAccounts([
            testTokenAccountPubkey,
            testTokenMintPubkey,
            payerKeypair.publicKey,
            newAccount,
          ])
        ).value;
        expect(accounts.length).to.eq(4);

        const parsedTokenAccount = accounts[0];
        if (parsedTokenAccount) {
          const data = parsedTokenAccount.data;
          if (Buffer.isBuffer(data)) {
            expect(Buffer.isBuffer(data)).to.eq(false);
          } else {
            expect(data.program).to.eq('spl-token');
            expect(data.parsed).to.be.ok;
          }
        } else {
          expect(parsedTokenAccount).to.be.ok;
        }

        const parsedTokenMint = accounts[1];
        if (parsedTokenMint) {
          const data = parsedTokenMint.data;
          if (Buffer.isBuffer(data)) {
            expect(Buffer.isBuffer(data)).to.eq(false);
          } else {
            expect(data.program).to.eq('spl-token');
            expect(data.parsed).to.be.ok;
          }
        } else {
          expect(parsedTokenMint).to.be.ok;
        }

        const unparsedPayerAccount = accounts[2];
        if (unparsedPayerAccount) {
          const data = unparsedPayerAccount.data;
          expect(Buffer.isBuffer(data)).to.be.true;
        } else {
          expect(unparsedPayerAccount).to.be.ok;
        }

        const unknownAccount = accounts[3];
        expect(unknownAccount).to.not.be.ok;
      });

      it('get parsed token program accounts', async () => {
        const tokenAccounts = await connection.getParsedProgramAccounts(
          TOKEN_PROGRAM_ID,
        );
        tokenAccounts.forEach(({account}) => {
          expect(account.owner).to.eql(TOKEN_PROGRAM_ID);
          const data = account.data;
          if (Buffer.isBuffer(data)) {
            expect(Buffer.isBuffer(data)).to.eq(false);
          } else {
            expect(data.parsed).to.be.ok;
            expect(data.program).to.eq('spl-token');
          }
        });
      });

      it('get parsed token accounts by owner', async () => {
        const tokenAccounts = (
          await connection.getParsedTokenAccountsByOwner(
            testOwnerKeypair.publicKey,
            {
              mint: testTokenMintPubkey,
            },
          )
        ).value;
        tokenAccounts.forEach(({account}) => {
          expect(account.owner).to.eql(TOKEN_PROGRAM_ID);
          const data = account.data;
          if (Buffer.isBuffer(data)) {
            expect(Buffer.isBuffer(data)).to.eq(false);
          } else {
            expect(data.parsed).to.be.ok;
            expect(data.program).to.eq('spl-token');
          }
        });
      });

      it('get token accounts by owner', async () => {
        const accountsWithMintFilter = (
          await connection.getTokenAccountsByOwner(testOwnerKeypair.publicKey, {
            mint: testTokenMintPubkey,
          })
        ).value;
        expect(accountsWithMintFilter).to.have.length(2);

        const accountsWithProgramFilter = (
          await connection.getTokenAccountsByOwner(testOwnerKeypair.publicKey, {
            programId: TOKEN_PROGRAM_ID,
          })
        ).value;
        expect(accountsWithProgramFilter).to.have.length(3);

        const noAccounts = (
          await connection.getTokenAccountsByOwner(newAccount, {
            mint: testTokenMintPubkey,
          })
        ).value;
        expect(noAccounts).to.have.length(0);

        await expect(
          connection.getTokenAccountsByOwner(testOwnerKeypair.publicKey, {
            mint: newAccount,
          }),
        ).to.be.rejected;

        await expect(
          connection.getTokenAccountsByOwner(testOwnerKeypair.publicKey, {
            programId: newAccount,
          }),
        ).to.be.rejected;
      });
    });

    it('consistent preflightCommitment', async () => {
      const connection = new Connection(url, 'singleGossip');
      const sender = Keypair.generate();
      const recipient = Keypair.generate();
      let signature = await connection.requestAirdrop(
        sender.publicKey,
        2 * LAMPORTS_PER_SOL,
      );
      await connection.confirmTransaction(signature, 'singleGossip');
      const transaction = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: sender.publicKey,
          toPubkey: recipient.publicKey,
          lamports: LAMPORTS_PER_SOL,
        }),
      );
      await sendAndConfirmTransaction(connection, transaction, [sender]);
    });
  }

  it('get largest accounts', async () => {
    await mockRpcResponse({
      method: 'getLargestAccounts',
      params: [],
      value: new Array(20).fill(0).map(() => ({
        address: Keypair.generate().publicKey.toBase58(),
        lamports: 1000,
      })),
      withContext: true,
    });

    const largestAccounts = (await connection.getLargestAccounts()).value;
    expect(largestAccounts).to.have.length(20);
  });

  it('stake activation should throw when called for not delegated account', async () => {
    const publicKey = Keypair.generate().publicKey;
    await mockRpcResponse({
      method: 'getStakeActivation',
      params: [publicKey.toBase58(), {}],
      error: {message: 'account not delegated'},
    });

    await expect(connection.getStakeActivation(publicKey)).to.be.rejected;
  });

  if (process.env.TEST_LIVE) {
    it('stake activation should return activating for new accounts', async () => {
      // todo: use `Connection.getMinimumStakeDelegation` when implemented
      const MIN_STAKE_DELEGATION = LAMPORTS_PER_SOL;
      const STAKE_ACCOUNT_MIN_BALANCE =
        await connection.getMinimumBalanceForRentExemption(StakeProgram.space);

      const voteAccounts = await connection.getVoteAccounts();
      const voteAccount = voteAccounts.current.concat(
        voteAccounts.delinquent,
      )[0];
      const votePubkey = new PublicKey(voteAccount.votePubkey);

      const authorized = Keypair.generate();
      let signature = await connection.requestAirdrop(
        authorized.publicKey,
        2 * LAMPORTS_PER_SOL,
      );
      await connection.confirmTransaction(signature, 'confirmed');

      const newStakeAccount = Keypair.generate();
      let createAndInitialize = StakeProgram.createAccount({
        fromPubkey: authorized.publicKey,
        stakePubkey: newStakeAccount.publicKey,
        authorized: new Authorized(authorized.publicKey, authorized.publicKey),
        lockup: new Lockup(0, 0, new PublicKey(0)),
        lamports: STAKE_ACCOUNT_MIN_BALANCE + MIN_STAKE_DELEGATION,
      });

      await sendAndConfirmTransaction(
        connection,
        createAndInitialize,
        [authorized, newStakeAccount],
        {
          preflightCommitment: 'confirmed',
          commitment: 'confirmed',
        },
      );
      let delegation = StakeProgram.delegate({
        stakePubkey: newStakeAccount.publicKey,
        authorizedPubkey: authorized.publicKey,
        votePubkey,
      });
      await sendAndConfirmTransaction(connection, delegation, [authorized], {
        preflightCommitment: 'confirmed',
        commitment: 'confirmed',
      });

      const LARGE_EPOCH = 4000;
      await expect(
        connection.getStakeActivation(
          newStakeAccount.publicKey,
          'confirmed',
          LARGE_EPOCH,
        ),
      ).to.be.rejectedWith(
        `failed to get Stake Activation ${newStakeAccount.publicKey.toBase58()}: Invalid param: epoch ${LARGE_EPOCH} has not yet started`,
      );

      const activationState = await connection.getStakeActivation(
        newStakeAccount.publicKey,
        'confirmed',
      );
      expect(activationState.state).to.eq('activating');
      expect(activationState.inactive).to.eq(MIN_STAKE_DELEGATION);
      expect(activationState.active).to.eq(0);
    });
  }

  if (mockServer) {
    it('stake activation should only accept state with valid string literals', async () => {
      const publicKey = Keypair.generate().publicKey;

      const addStakeActivationMock = async (state: any) => {
        await mockRpcResponse({
          method: 'getStakeActivation',
          params: [publicKey.toBase58(), {}],
          value: {
            state: state,
            active: 0,
            inactive: 80,
          },
        });
      };

      await addStakeActivationMock('active');
      let activation = await connection.getStakeActivation(
        publicKey,
        'confirmed',
      );
      expect(activation.state).to.eq('active');
      expect(activation.active).to.eq(0);
      expect(activation.inactive).to.eq(80);

      await addStakeActivationMock('invalid');
      await expect(connection.getStakeActivation(publicKey, 'confirmed')).to.be
        .rejected;
    });
  }

  it('getVersion', async () => {
    await mockRpcResponse({
      method: 'getVersion',
      params: [],
      value: {'solana-core': '0.20.4'},
    });

    const version = await connection.getVersion();
    expect(version['solana-core']).to.be.ok;
  });

  it('getGenesisHash', async () => {
    await mockRpcResponse({
      method: 'getGenesisHash',
      params: [],
      value: 'GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC',
    });

    const genesisHash = await connection.getGenesisHash();
    expect(genesisHash).not.to.be.empty;
  });

  it('request airdrop', async () => {
    const account = Keypair.generate();

    await helpers.airdrop({
      connection,
      address: account.publicKey,
      amount: LAMPORTS_PER_SOL,
    });

    await mockRpcResponse({
      method: 'getBalance',
      params: [account.publicKey.toBase58(), {commitment: 'confirmed'}],
      value: LAMPORTS_PER_SOL,
      withContext: true,
    });

    const balance = await connection.getBalance(account.publicKey, 'confirmed');
    expect(balance).to.eq(LAMPORTS_PER_SOL);

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [
        account.publicKey.toBase58(),
        {commitment: 'confirmed', encoding: 'base64'},
      ],
      value: {
        owner: '11111111111111111111111111111111',
        lamports: LAMPORTS_PER_SOL,
        data: ['', 'base64'],
        executable: false,
        rentEpoch: 20,
      },
      withContext: true,
    });

    const accountInfo = await connection.getAccountInfo(
      account.publicKey,
      'confirmed',
    );
    if (accountInfo === null) {
      expect(accountInfo).not.to.be.null;
      return;
    }
    expect(accountInfo.lamports).to.eq(LAMPORTS_PER_SOL);
    expect(accountInfo.data).to.have.length(0);
    expect(accountInfo.owner).to.eql(SystemProgram.programId);

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [
        account.publicKey.toBase58(),
        {commitment: 'confirmed', encoding: 'jsonParsed'},
      ],
      value: {
        owner: '11111111111111111111111111111111',
        lamports: LAMPORTS_PER_SOL,
        data: ['', 'base64'],
        executable: false,
        rentEpoch: 20,
      },
      withContext: true,
    });

    const parsedAccountInfo = (
      await connection.getParsedAccountInfo(account.publicKey, 'confirmed')
    ).value;
    if (parsedAccountInfo === null) {
      expect(parsedAccountInfo).not.to.be.null;
      return;
    } else if ('parsed' in parsedAccountInfo.data) {
      expect(parsedAccountInfo.data.parsed).not.to.be.ok;
      return;
    }
    expect(parsedAccountInfo.lamports).to.eq(LAMPORTS_PER_SOL);
    expect(parsedAccountInfo.data).to.have.length(0);
    expect(parsedAccountInfo.owner).to.eql(SystemProgram.programId);
  });

  it('transaction failure', async () => {
    const payer = Keypair.generate();

    await helpers.airdrop({
      connection,
      address: payer.publicKey,
      amount: LAMPORTS_PER_SOL,
    });

    const newAccount = Keypair.generate();
    let transaction = new Transaction().add(
      SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: newAccount.publicKey,
        lamports: LAMPORTS_PER_SOL / 2,
        space: 0,
        programId: SystemProgram.programId,
      }),
    );

    await helpers.processTransaction({
      connection,
      transaction,
      signers: [payer, newAccount],
      commitment: 'confirmed',
    });

    // This should fail because the account is already created
    const expectedErr = {InstructionError: [0, {Custom: 0}]};
    const confirmResult = (
      await helpers.processTransaction({
        connection,
        transaction,
        signers: [payer, newAccount],
        commitment: 'confirmed',
        err: expectedErr,
      })
    ).value;
    expect(confirmResult.err).to.eql(expectedErr);

    invariant(transaction.signature);
    const signature = bs58.encode(transaction.signature);
    await mockRpcResponse({
      method: 'getSignatureStatuses',
      params: [[signature]],
      value: [
        {
          slot: 0,
          confirmations: 11,
          status: {Err: expectedErr},
          err: expectedErr,
        },
      ],
      withContext: true,
    });

    const response = (await connection.getSignatureStatus(signature)).value;
    verifySignatureStatus(response, expectedErr);
  });

  if (mockServer) {
    it('returnData on simulateTransaction', async () => {
      const tx = new Transaction();
      tx.feePayer = Keypair.generate().publicKey;

      const getLatestBlockhashResponse = {
        method: 'getLatestBlockhash',
        params: [],
        value: {
          blockhash: 'CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR',
          feeCalculator: {
            lamportsPerSignature: 5000,
          },
          lastValidBlockHeight: 51,
        },
        withContext: true,
      };
      const simulateTransactionResponse = {
        method: 'simulateTransaction',
        params: [],
        value: {
          err: null,
          accounts: null,
          logs: [
            'Program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri invoke [1]',
            'Program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri consumed 2366 of 1400000 compute units',
            'Program return: 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri KgAAAAAAAAA=',
            'Program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success',
          ],
          returnData: {
            data: ['KgAAAAAAAAA==', 'base64'],
            programId: '83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri',
          },
          unitsConsumed: 2366,
        },
        withContext: true,
      };
      await mockRpcResponse(getLatestBlockhashResponse);
      await mockRpcResponse(simulateTransactionResponse);
      const response = (await connection.simulateTransaction(tx)).value;
      expect(response.returnData).to.eql({
        data: ['KgAAAAAAAAA==', 'base64'],
        programId: '83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri',
      });
    });
  }

  if (process.env.TEST_LIVE) {
    it('getStakeMinimumDelegation', async () => {
      const {value} = await connection.getStakeMinimumDelegation();
      expect(value).to.be.a('number');
    });

    it('sendTransaction', async () => {
      const connection = new Connection(url, 'confirmed');
      const payer = Keypair.generate();

      await helpers.airdrop({
        connection,
        address: payer.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      const recentBlockhash = await (
        await helpers.latestBlockhash({connection})
      ).blockhash;

      const versionedTx = new VersionedTransaction(
        new Message({
          header: {
            numRequiredSignatures: 1,
            numReadonlySignedAccounts: 0,
            numReadonlyUnsignedAccounts: 0,
          },
          recentBlockhash,
          instructions: [],
          accountKeys: [payer.publicKey.toBase58()],
        }),
      );

      versionedTx.sign([payer]);
      await connection.sendTransaction(versionedTx);
    });

    it('simulateTransaction', async () => {
      const connection = new Connection(url, 'confirmed');
      const payer = Keypair.generate();

      await helpers.airdrop({
        connection,
        address: payer.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      const recentBlockhash = await (
        await helpers.latestBlockhash({connection})
      ).blockhash;

      const versionedTx = new VersionedTransaction(
        new Message({
          header: {
            numRequiredSignatures: 1,
            numReadonlySignedAccounts: 0,
            numReadonlyUnsignedAccounts: 0,
          },
          recentBlockhash,
          instructions: [],
          accountKeys: [payer.publicKey.toBase58()],
        }),
      );

      const response = await connection.simulateTransaction(versionedTx, {
        accounts: {
          encoding: 'base64',
          addresses: [payer.publicKey.toBase58()],
        },
      });
      expect(response.value.err).to.be.null;
      expect(response.value.accounts).to.eql([
        {
          data: ['', 'base64'],
          executable: false,
          lamports: LAMPORTS_PER_SOL - 5000,
          owner: SystemProgram.programId.toBase58(),
          rentEpoch: 0,
          space: 0,
        },
      ]);
    });

    it('simulate transaction with message', async () => {
      connection._commitment = 'confirmed';

      const account1 = Keypair.generate();
      const account2 = Keypair.generate();

      await helpers.airdrop({
        connection,
        address: account1.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      await helpers.airdrop({
        connection,
        address: account2.publicKey,
        amount: LAMPORTS_PER_SOL,
      });

      const recentBlockhash = await (
        await helpers.latestBlockhash({connection})
      ).blockhash;
      const message = new Message({
        accountKeys: [
          account1.publicKey.toString(),
          account2.publicKey.toString(),
          'Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo',
        ],
        header: {
          numReadonlySignedAccounts: 1,
          numReadonlyUnsignedAccounts: 2,
          numRequiredSignatures: 1,
        },
        instructions: [
          {
            accounts: [0, 1],
            data: bs58.encode(Buffer.alloc(5).fill(9)),
            programIdIndex: 2,
          },
        ],
        recentBlockhash,
      });

      const results1 = await connection.simulateTransaction(
        message,
        [account1],
        true,
      );

      expect(results1.value.accounts).lengthOf(2);

      const results2 = await connection.simulateTransaction(
        message,
        [account1],
        [
          account1.publicKey,
          new PublicKey('Missing111111111111111111111111111111111111'),
        ],
      );

      expect(results2.value.accounts).lengthOf(2);
      if (results2.value.accounts) {
        expect(results2.value.accounts[1]).to.be.null;
      }
    }).timeout(10000);

    it('transaction', async () => {
      connection._commitment = 'confirmed';

      const accountFrom = Keypair.generate();
      const accountTo = Keypair.generate();
      const minimumAmount = await connection.getMinimumBalanceForRentExemption(
        0,
      );

      await helpers.airdrop({
        connection,
        address: accountFrom.publicKey,
        amount: minimumAmount + 100010,
      });
      await helpers.airdrop({
        connection,
        address: accountTo.publicKey,
        amount: minimumAmount,
      });

      const transaction = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: accountFrom.publicKey,
          toPubkey: accountTo.publicKey,
          lamports: 10,
        }),
      );

      const signature = await sendAndConfirmTransaction(
        connection,
        transaction,
        [accountFrom],
        {preflightCommitment: 'confirmed'},
      );

      // Send again and ensure that new blockhash is used
      const lastFetch = Date.now();
      const transaction2 = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: accountFrom.publicKey,
          toPubkey: accountTo.publicKey,
          lamports: 10,
        }),
      );

      const signature2 = await sendAndConfirmTransaction(
        connection,
        transaction2,
        [accountFrom],
        {preflightCommitment: 'confirmed'},
      );

      expect(signature).not.to.eq(signature2);
      expect(transaction.recentBlockhash).not.to.eq(
        transaction2.recentBlockhash,
      );

      // Send new transaction and ensure that same blockhash is used
      const transaction3 = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: accountFrom.publicKey,
          toPubkey: accountTo.publicKey,
          lamports: 9,
        }),
      );
      await sendAndConfirmTransaction(connection, transaction3, [accountFrom], {
        preflightCommitment: 'confirmed',
      });
      expect(transaction2.recentBlockhash).to.eq(transaction3.recentBlockhash);

      // Sleep until blockhash cache times out
      await sleep(
        Math.max(
          0,
          1000 + BLOCKHASH_CACHE_TIMEOUT_MS - (Date.now() - lastFetch),
        ),
      );

      const transaction4 = new Transaction().add(
        SystemProgram.transfer({
          fromPubkey: accountFrom.publicKey,
          toPubkey: accountTo.publicKey,
          lamports: 13,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction4, [accountFrom], {
        preflightCommitment: 'confirmed',
      });

      expect(transaction4.recentBlockhash).not.to.eq(
        transaction3.recentBlockhash,
      );

      // accountFrom may have less than 100000 due to transaction fees
      const balance = await connection.getBalance(accountFrom.publicKey);
      expect(balance).to.be.greaterThan(0);
      expect(balance).to.be.at.most(minimumAmount + 100000);
      expect(await connection.getBalance(accountTo.publicKey)).to.eq(
        minimumAmount + 42,
      );
    }).timeout(45 * 1000); // waits 30s for cache timeout

    it('multi-instruction transaction', async () => {
      connection._commitment = 'confirmed';

      const accountFrom = Keypair.generate();
      const accountTo = Keypair.generate();

      let signature = await connection.requestAirdrop(
        accountFrom.publicKey,
        LAMPORTS_PER_SOL,
      );
      await connection.confirmTransaction(signature);
      expect(await connection.getBalance(accountFrom.publicKey)).to.eq(
        LAMPORTS_PER_SOL,
      );

      const minimumAmount = await connection.getMinimumBalanceForRentExemption(
        0,
      );

      signature = await connection.requestAirdrop(
        accountTo.publicKey,
        minimumAmount + 21,
      );
      await connection.confirmTransaction(signature);
      expect(await connection.getBalance(accountTo.publicKey)).to.eq(
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

      await connection.confirmTransaction({
        blockhash: transaction.recentBlockhash!,
        lastValidBlockHeight: transaction.lastValidBlockHeight!,
        signature: signature,
      });

      const response = (await connection.getSignatureStatus(signature)).value;
      if (response !== null) {
        expect(typeof response.slot).to.eq('number');
        expect(response.err).to.be.null;
      } else {
        expect(response).not.to.be.null;
      }

      // accountFrom may have less than LAMPORTS_PER_SOL due to transaction fees
      expect(
        await connection.getBalance(accountFrom.publicKey),
      ).to.be.greaterThan(0);
      expect(await connection.getBalance(accountFrom.publicKey)).to.be.at.most(
        LAMPORTS_PER_SOL,
      );

      expect(await connection.getBalance(accountTo.publicKey)).to.eq(
        minimumAmount + 21,
      );
    });

    describe('given an open websocket connection', () => {
      beforeEach(async () => {
        // Open the socket connection and wait for it to become pingable.
        connection._rpcWebSocket.connect();
        // eslint-disable-next-line no-constant-condition
        while (true) {
          try {
            await connection._rpcWebSocket.notify('ping');
            break;
            // eslint-disable-next-line no-empty
          } catch (_err) {}
          await sleep(100);
        }
      });

      it('account change notification', async () => {
        const connection = new Connection(url, 'confirmed');
        const owner = Keypair.generate();

        let subscriptionId: number | undefined;
        try {
          const accountInfoPromise = new Promise<AccountInfo<Buffer>>(
            resolve => {
              subscriptionId = connection.onAccountChange(
                owner.publicKey,
                resolve,
                'confirmed',
              );
            },
          );
          connection.requestAirdrop(owner.publicKey, LAMPORTS_PER_SOL);
          const accountInfo = await accountInfoPromise;
          expect(accountInfo.lamports).to.eq(LAMPORTS_PER_SOL);
          expect(accountInfo.owner.equals(SystemProgram.programId)).to.be.true;
        } finally {
          if (subscriptionId != null) {
            await connection.removeAccountChangeListener(subscriptionId);
          }
        }
      });

      it('program account change notification', async () => {
        connection._commitment = 'confirmed';

        const owner = Keypair.generate();
        const programAccount = Keypair.generate();
        const balanceNeeded =
          await connection.getMinimumBalanceForRentExemption(0);

        let subscriptionId: number | undefined;
        try {
          const keyedAccountInfoPromise = new Promise<KeyedAccountInfo>(
            resolve => {
              subscriptionId = connection.onProgramAccountChange(
                SystemProgram.programId,
                resolve,
              );
            },
          );

          await helpers.airdrop({
            connection,
            address: owner.publicKey,
            amount: LAMPORTS_PER_SOL,
          });

          const transaction = new Transaction().add(
            SystemProgram.transfer({
              fromPubkey: owner.publicKey,
              toPubkey: programAccount.publicKey,
              lamports: balanceNeeded,
            }),
          );
          await sendAndConfirmTransaction(connection, transaction, [owner], {
            commitment: 'confirmed',
          });

          const keyedAccountInfo = await keyedAccountInfoPromise;
          if (keyedAccountInfo.accountId.equals(programAccount.publicKey)) {
            expect(keyedAccountInfo.accountInfo.lamports).to.eq(balanceNeeded);
            expect(
              keyedAccountInfo.accountInfo.owner.equals(
                SystemProgram.programId,
              ),
            ).to.be.true;
          }
        } finally {
          if (subscriptionId != null) {
            await connection.removeProgramAccountChangeListener(subscriptionId);
          }
        }
      });

      it('slot notification', async () => {
        let subscriptionId: number | undefined;
        try {
          const notifiedSlotInfo = await new Promise<SlotInfo>(resolve => {
            subscriptionId = connection.onSlotChange(resolve);
          });
          expect(notifiedSlotInfo.parent).to.be.at.least(0);
          expect(notifiedSlotInfo.root).to.be.at.least(0);
          expect(notifiedSlotInfo.slot).to.be.at.least(1);
        } finally {
          if (subscriptionId != null) {
            await connection.removeSlotChangeListener(subscriptionId);
          }
        }
      });

      it('root notification', async () => {
        let subscriptionId: number | undefined;
        try {
          const atLeastTwoRoots = await new Promise<number[]>(resolve => {
            const roots: number[] = [];
            subscriptionId = connection.onRootChange(root => {
              if (roots.length === 2) {
                return;
              }
              roots.push(root);
              if (roots.length === 2) {
                // Collect at least two, then resolve.
                resolve(roots);
              }
            });
          });
          expect(atLeastTwoRoots[1]).to.be.greaterThan(atLeastTwoRoots[0]);
        } finally {
          if (subscriptionId != null) {
            await connection.removeRootChangeListener(subscriptionId);
          }
        }
      });

      it('signature notification', async () => {
        const owner = Keypair.generate();
        const signature = await connection.requestAirdrop(
          owner.publicKey,
          LAMPORTS_PER_SOL,
        );
        const signatureResult = await new Promise<SignatureResult>(resolve => {
          // NOTE: Signature subscriptions auto-remove themselves, so there's no
          // need to track the subscription id and remove it when the test ends.
          connection.onSignature(signature, resolve, 'processed');
        });
        expect(signatureResult.err).to.be.null;
      });

      it('logs notification', async () => {
        let subscriptionId: number | undefined;
        const owner = Keypair.generate();
        try {
          const logPromise = new Promise<[Logs, Context]>(resolve => {
            subscriptionId = connection.onLogs(
              owner.publicKey,
              (logs, ctx) => {
                if (!logs.err) {
                  resolve([logs, ctx]);
                }
              },
              'processed',
            );
          });

          // Execute a transaction so that we can pickup its logs.
          await connection.requestAirdrop(owner.publicKey, LAMPORTS_PER_SOL);

          const [logsRes, ctx] = await logPromise;
          expect(ctx.slot).to.be.greaterThan(0);
          expect(logsRes.logs.length).to.eq(2);
          expect(logsRes.logs[0]).to.eq(
            'Program 11111111111111111111111111111111 invoke [1]',
          );
          expect(logsRes.logs[1]).to.eq(
            'Program 11111111111111111111111111111111 success',
          );
        } finally {
          if (subscriptionId != null) {
            await connection.removeOnLogsListener(subscriptionId);
          }
        }
      });
    });

    it('https request', async () => {
      const connection = new Connection('https://api.mainnet-beta.solana.com');
      const version = await connection.getVersion();
      expect(version['solana-core']).to.be.ok;
    }).timeout(20 * 1000);

    let lookupTableKey: PublicKey;
    const lookupTableAddresses = new Array(10)
      .fill(0)
      .map(() => Keypair.generate().publicKey);

    describe('address lookup table program', () => {
      const connection = new Connection(url);
      const payer = Keypair.generate();

      before(async () => {
        await helpers.airdrop({
          connection,
          address: payer.publicKey,
          amount: 10 * LAMPORTS_PER_SOL,
        });
      });

      it('createLookupTable', async () => {
        const recentSlot = await connection.getSlot('finalized');

        let createIx: TransactionInstruction;
        [createIx, lookupTableKey] =
          AddressLookupTableProgram.createLookupTable({
            recentSlot,
            payer: payer.publicKey,
            authority: payer.publicKey,
          });

        await helpers.processTransaction({
          connection,
          transaction: new Transaction().add(createIx),
          signers: [payer],
          commitment: 'processed',
        });
      });

      it('extendLookupTable', async () => {
        const transaction = new Transaction().add(
          AddressLookupTableProgram.extendLookupTable({
            lookupTable: lookupTableKey,
            addresses: lookupTableAddresses,
            authority: payer.publicKey,
            payer: payer.publicKey,
          }),
        );

        await helpers.processTransaction({
          connection,
          transaction,
          signers: [payer],
          commitment: 'processed',
        });
      });

      it('freezeLookupTable', async () => {
        const transaction = new Transaction().add(
          AddressLookupTableProgram.freezeLookupTable({
            lookupTable: lookupTableKey,
            authority: payer.publicKey,
          }),
        );

        await helpers.processTransaction({
          connection,
          transaction,
          signers: [payer],
          commitment: 'processed',
        });
      });

      it('getAddressLookupTable', async () => {
        const lookupTableResponse = await connection.getAddressLookupTable(
          lookupTableKey,
          {
            commitment: 'processed',
          },
        );
        const lookupTableAccount = lookupTableResponse.value;
        if (!lookupTableAccount) {
          expect(lookupTableAccount).to.be.ok;
          return;
        }
        expect(lookupTableAccount.isActive()).to.be.true;
        expect(lookupTableAccount.state.authority).to.be.undefined;
        expect(lookupTableAccount.state.addresses).to.eql(lookupTableAddresses);
      });
    });

    describe('v0 transaction', () => {
      const connection = new Connection(url);
      const payer = Keypair.generate();

      before(async () => {
        await helpers.airdrop({
          connection,
          address: payer.publicKey,
          amount: 10 * LAMPORTS_PER_SOL,
        });
      });

      // wait for lookup table to be usable
      before(async () => {
        const lookupTableResponse = await connection.getAddressLookupTable(
          lookupTableKey,
          {
            commitment: 'processed',
          },
        );

        const lookupTableAccount = lookupTableResponse.value;
        if (!lookupTableAccount) {
          expect(lookupTableAccount).to.be.ok;
          return;
        }

        // eslint-disable-next-line no-constant-condition
        while (true) {
          const latestSlot = await connection.getSlot('confirmed');
          if (latestSlot > lookupTableAccount.state.lastExtendedSlot) {
            break;
          } else {
            console.log('Waiting for next slot...');
            await sleep(500);
          }
        }
      });

      let signature: TransactionSignature;
      let addressTableLookups: MessageAddressTableLookup[];
      it('send and confirm', async () => {
        const {blockhash, lastValidBlockHeight} =
          await connection.getLatestBlockhash();
        const transferIxData = encodeData(SYSTEM_INSTRUCTION_LAYOUTS.Transfer, {
          lamports: BigInt(LAMPORTS_PER_SOL),
        });
        addressTableLookups = [
          {
            accountKey: lookupTableKey,
            writableIndexes: [0],
            readonlyIndexes: [],
          },
        ];
        const transaction = new VersionedTransaction(
          new MessageV0({
            header: {
              numRequiredSignatures: 1,
              numReadonlySignedAccounts: 0,
              numReadonlyUnsignedAccounts: 1,
            },
            staticAccountKeys: [payer.publicKey, SystemProgram.programId],
            recentBlockhash: blockhash,
            compiledInstructions: [
              {
                programIdIndex: 1,
                accountKeyIndexes: [0, 2],
                data: transferIxData,
              },
            ],
            addressTableLookups,
          }),
        );
        transaction.sign([payer]);
        signature = bs58.encode(transaction.signatures[0]);
        const serializedTransaction = transaction.serialize();
        await connection.sendRawTransaction(serializedTransaction, {
          preflightCommitment: 'confirmed',
        });

        await connection.confirmTransaction(
          {
            signature,
            blockhash,
            lastValidBlockHeight,
          },
          'confirmed',
        );

        const transferToKey = lookupTableAddresses[0];
        const transferToAccount = await connection.getAccountInfo(
          transferToKey,
          {commitment: 'confirmed', dataSlice: {length: 0, offset: 0}},
        );
        expect(transferToAccount?.data.length).to.be.eq(0);
        expect(transferToAccount?.lamports).to.be.eq(LAMPORTS_PER_SOL);
      });

      it('getTransaction (failure)', async () => {
        await expect(
          connection.getTransaction(signature, {
            commitment: 'confirmed',
          }),
        ).to.be.rejectedWith(
          'failed to get transaction: Transaction version (0) is not supported',
        );
      });

      let transactionSlot: number;
      it('getTransaction', async () => {
        // fetch v0 transaction
        const fetchedTransaction = await connection.getTransaction(signature, {
          commitment: 'confirmed',
          maxSupportedTransactionVersion: 0,
        });
        if (fetchedTransaction === null) {
          expect(fetchedTransaction).to.not.be.null;
          return;
        }
        transactionSlot = fetchedTransaction.slot;
        expect(fetchedTransaction.version).to.eq(0);
        expect(fetchedTransaction.meta?.loadedAddresses).to.eql({
          readonly: [],
          writable: [lookupTableAddresses[0]],
        });
        expect(fetchedTransaction.meta?.computeUnitsConsumed).to.not.be
          .undefined;
        expect(
          fetchedTransaction.transaction.message.addressTableLookups,
        ).to.eql(addressTableLookups);
      });

      it('getParsedTransaction (failure)', async () => {
        await expect(
          connection.getParsedTransaction(signature, {
            commitment: 'confirmed',
          }),
        ).to.be.rejectedWith(
          'failed to get transaction: Transaction version (0) is not supported',
        );
      });

      it('getParsedTransaction', async () => {
        const parsedTransaction = await connection.getParsedTransaction(
          signature,
          {
            commitment: 'confirmed',
            maxSupportedTransactionVersion: 0,
          },
        );
        expect(parsedTransaction).to.not.be.null;
        expect(parsedTransaction?.version).to.eq(0);
        // loaded addresses are not returned for parsed transactions
        expect(parsedTransaction?.meta?.loadedAddresses).to.be.undefined;
        expect(parsedTransaction?.meta?.computeUnitsConsumed).to.not.be
          .undefined;
        expect(
          parsedTransaction?.transaction.message.addressTableLookups,
        ).to.eql(addressTableLookups);
        expect(parsedTransaction?.transaction.message.accountKeys).to.eql([
          {
            pubkey: payer.publicKey,
            signer: true,
            writable: true,
            source: 'transaction',
          },
          {
            pubkey: SystemProgram.programId,
            signer: false,
            writable: false,
            source: 'transaction',
          },
          {
            pubkey: lookupTableAddresses[0],
            signer: false,
            writable: true,
            source: 'lookupTable',
          },
        ]);
      });

      it('getBlock (failure)', async () => {
        await expect(
          connection.getBlock(transactionSlot, {
            maxSupportedTransactionVersion: undefined,
            commitment: 'confirmed',
          }),
        ).to.be.rejectedWith(
          'failed to get confirmed block: Transaction version (0) is not supported',
        );
      });

      it('getBlock', async () => {
        const block = await connection.getBlock(transactionSlot, {
          maxSupportedTransactionVersion: 0,
          commitment: 'confirmed',
        });
        expect(block).to.not.be.null;
        if (block === null) throw new Error(); // unreachable

        let foundTx = false;
        for (const tx of block.transactions) {
          if (tx.transaction.signatures[0] === signature) {
            foundTx = true;
            expect(tx.version).to.eq(0);
          }
        }
        expect(foundTx).to.be.true;
      });

      it('getParsedBlock', async () => {
        const block = await connection.getParsedBlock(transactionSlot, {
          maxSupportedTransactionVersion: 0,
          commitment: 'confirmed',
        });
        expect(block).to.not.be.null;
        if (block === null) throw new Error(); // unreachable

        let foundTx = false;
        for (const tx of block.transactions) {
          if (tx.transaction.signatures[0] === signature) {
            foundTx = true;
            expect(tx.version).to.eq(0);
          }
        }
        expect(foundTx).to.be.true;
      });
    }).timeout(5 * 1000);
  }
});
