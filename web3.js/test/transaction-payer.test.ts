import base58 from 'bs58';
import invariant from 'assert';
import {expect} from 'chai';

import {
  Account,
  Connection,
  Transaction,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from '../src';
import {MOCK_PORT, url} from './url';
import {helpers, mockRpcResponse, mockServer} from './mocks/rpc-http';
import {stubRpcWebSocket, restoreRpcWebSocket} from './mocks/rpc-websockets';

describe('Transaction Payer', () => {
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

  it('transaction-payer', async () => {
    const accountPayer = new Account();
    const accountFrom = new Account();
    const accountTo = new Account();

    await helpers.airdrop({
      connection,
      address: accountPayer.publicKey,
      amount: LAMPORTS_PER_SOL,
    });

    await mockRpcResponse({
      method: 'getMinimumBalanceForRentExemption',
      params: [0],
      value: 50,
    });

    const minimumAmount = await connection.getMinimumBalanceForRentExemption(0);

    await helpers.airdrop({
      connection,
      address: accountFrom.publicKey,
      amount: minimumAmount + 12,
    });

    await helpers.airdrop({
      connection,
      address: accountTo.publicKey,
      amount: minimumAmount + 21,
    });

    const transaction = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: accountFrom.publicKey,
        toPubkey: accountTo.publicKey,
        lamports: 10,
      }),
    );

    await helpers.processTransaction({
      connection,
      transaction,
      signers: [accountPayer, accountFrom],
      commitment: 'confirmed',
    });

    invariant(transaction.signature);
    const signature = base58.encode(transaction.signature);

    await mockRpcResponse({
      method: 'getSignatureStatuses',
      params: [[signature]],
      value: [
        {
          slot: 0,
          confirmations: 11,
          status: {Ok: null},
          err: null,
        },
      ],
      withContext: true,
    });
    const {value} = await connection.getSignatureStatus(signature);
    if (value !== null) {
      expect(typeof value.slot).to.eq('number');
      expect(value.err).to.be.null;
    } else {
      expect(value).not.to.be.null;
    }

    await mockRpcResponse({
      method: 'getBalance',
      params: [accountPayer.publicKey.toBase58(), {commitment: 'confirmed'}],
      value: LAMPORTS_PER_SOL - 1,
      withContext: true,
    });

    // accountPayer should be less than LAMPORTS_PER_SOL as it paid for the transaction
    // (exact amount less depends on the current cluster fees)
    const balance = await connection.getBalance(
      accountPayer.publicKey,
      'confirmed',
    );
    expect(balance).to.be.greaterThan(0);
    expect(balance).to.be.at.most(LAMPORTS_PER_SOL);

    // accountFrom should have exactly 2, since it didn't pay for the transaction
    await mockRpcResponse({
      method: 'getBalance',
      params: [accountFrom.publicKey.toBase58(), {commitment: 'confirmed'}],
      value: minimumAmount + 2,
      withContext: true,
    });
    expect(
      await connection.getBalance(accountFrom.publicKey, 'confirmed'),
    ).to.eq(minimumAmount + 2);
  });
});
