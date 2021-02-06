// @flow

import bs58 from 'bs58';
import {Buffer} from 'buffer';
import {expect} from 'chai';

import {
  Account,
  Connection,
  SystemProgram,
  Transaction,
  PublicKey,
} from '../src';
import {NONCE_ACCOUNT_LENGTH} from '../src/nonce-account';
import {MOCK_PORT, url} from './url';
import {helpers, mockRpcResponse, mockServer} from './mocks/rpc-http';
import {stubRpcWebSocket, restoreRpcWebSocket} from './mocks/rpc-websockets';

const expectedData = (authorizedPubkey: PublicKey): [string, string] => {
  const expectedData = Buffer.alloc(NONCE_ACCOUNT_LENGTH);
  expectedData.writeInt32LE(0, 0); // Version, 4 bytes
  expectedData.writeInt32LE(1, 4); // State, 4 bytes
  authorizedPubkey.toBuffer().copy(expectedData, 8); // authorizedPubkey, 32 bytes
  const mockNonce = new Account();
  mockNonce.publicKey.toBuffer().copy(expectedData, 40); // Hash, 32 bytes
  expectedData.writeUInt16LE(5000, 72); // feeCalculator, 8 bytes
  return [expectedData.toString('base64'), 'base64'];
};

describe('Nonce', () => {
  let connection: Connection;
  beforeEach(() => {
    connection = new Connection(url);
  });

  if (!process.env.TEST_LIVE) {
    beforeEach(() => {
      mockServer.start(MOCK_PORT);
      stubRpcWebSocket(connection);
    });

    afterEach(() => {
      mockServer.stop();
      restoreRpcWebSocket(connection);
    });
  }

  it('create and query nonce account', async () => {
    const from = new Account();
    const nonceAccount = new Account();

    await mockRpcResponse({
      method: 'getMinimumBalanceForRentExemption',
      params: [NONCE_ACCOUNT_LENGTH],
      value: 50,
    });

    const minimumAmount = await connection.getMinimumBalanceForRentExemption(
      NONCE_ACCOUNT_LENGTH,
    );

    await helpers.airdrop({
      connection,
      address: from.publicKey,
      amount: minimumAmount * 2,
    });

    const transaction = new Transaction().add(
      SystemProgram.createNonceAccount({
        fromPubkey: from.publicKey,
        noncePubkey: nonceAccount.publicKey,
        authorizedPubkey: from.publicKey,
        lamports: minimumAmount,
      }),
    );

    await helpers.processTransaction({
      connection,
      transaction,
      signers: [from, nonceAccount],
      commitment: 'singleGossip',
    });

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [
        nonceAccount.publicKey.toBase58(),
        {encoding: 'base64', commitment: 'singleGossip'},
      ],
      value: {
        owner: '11111111111111111111111111111111',
        lamports: minimumAmount,
        data: expectedData(from.publicKey),
        executable: false,
      },
      withContext: true,
    });

    const nonceAccountData = await connection.getNonce(
      nonceAccount.publicKey,
      'singleGossip',
    );
    if (nonceAccountData === null) {
      expect(nonceAccountData).not.to.be.null;
      return;
    }
    expect(nonceAccountData.authorizedPubkey).to.eql(from.publicKey);
    expect(bs58.decode(nonceAccountData.nonce).length).to.be.greaterThan(30);
  });

  it('create and query nonce account with seed', async () => {
    const from = new Account();
    const seed = 'seed';
    const noncePubkey = await PublicKey.createWithSeed(
      from.publicKey,
      seed,
      SystemProgram.programId,
    );

    await mockRpcResponse({
      method: 'getMinimumBalanceForRentExemption',
      params: [NONCE_ACCOUNT_LENGTH],
      value: 50,
    });

    const minimumAmount = await connection.getMinimumBalanceForRentExemption(
      NONCE_ACCOUNT_LENGTH,
    );

    await helpers.airdrop({
      connection,
      address: from.publicKey,
      amount: minimumAmount * 2,
    });

    const transaction = new Transaction().add(
      SystemProgram.createNonceAccount({
        fromPubkey: from.publicKey,
        noncePubkey: noncePubkey,
        basePubkey: from.publicKey,
        seed,
        authorizedPubkey: from.publicKey,
        lamports: minimumAmount,
      }),
    );

    await helpers.processTransaction({
      connection,
      transaction,
      signers: [from],
      commitment: 'singleGossip',
    });

    await mockRpcResponse({
      method: 'getAccountInfo',
      params: [
        noncePubkey.toBase58(),
        {encoding: 'base64', commitment: 'singleGossip'},
      ],
      value: {
        owner: '11111111111111111111111111111111',
        lamports: minimumAmount,
        data: expectedData(from.publicKey),
        executable: false,
      },
      withContext: true,
    });

    const nonceAccountData = await connection.getNonce(
      noncePubkey,
      'singleGossip',
    );
    if (nonceAccountData === null) {
      expect(nonceAccountData).not.to.be.null;
      return;
    }
    expect(nonceAccountData.authorizedPubkey).to.eql(from.publicKey);
    expect(bs58.decode(nonceAccountData.nonce).length).to.be.greaterThan(30);
  });
});
