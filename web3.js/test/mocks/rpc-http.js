// @flow

import bs58 from 'bs58';
import BN from 'bn.js';
import * as mockttp from 'mockttp';

import {mockRpcMessage} from './rpc-websockets';
import {Account, Connection, PublicKey, Transaction} from '../../src';
import type {Commitment} from '../../src/connection';

export const mockServer: mockttp.Mockttp =
  process.env.TEST_LIVE || mockttp.getLocal();

let uniqueCounter = 0;
export const uniqueSignature = () => {
  return bs58.encode(new BN(++uniqueCounter).toArray(null, 64));
};
export const uniqueBlockhash = () => {
  return bs58.encode(new BN(++uniqueCounter).toArray(null, 32));
};

export const mockErrorMessage = 'Invalid';
export const mockErrorResponse = {
  code: -32602,
  message: mockErrorMessage,
};

export const mockRpcResponse = async ({
  method,
  params,
  value,
  error,
  withContext,
}: {
  method: string,
  params: Array<any>,
  value?: any,
  error?: any,
  withContext?: boolean,
}) => {
  if (process.env.TEST_LIVE) return;

  let result = value;
  if (withContext) {
    result = {
      context: {
        slot: 11,
      },
      value,
    };
  }

  await mockServer
    .post('/')
    .withJsonBodyIncluding({
      jsonrpc: '2.0',
      method,
      params,
    })
    .thenReply(
      200,
      JSON.stringify({
        jsonrpc: '2.0',
        id: '',
        error,
        result,
      }),
    );
};

const recentBlockhash = async ({
  connection,
  commitment,
}: {
  connection: Connection,
  commitment?: Commitment,
}) => {
  const blockhash = uniqueBlockhash();
  const params = [];
  if (commitment) {
    params.push({commitment});
  }

  await mockRpcResponse({
    method: 'getRecentBlockhash',
    params,
    value: {
      blockhash,
      feeCalculator: {
        lamportsPerSignature: 42,
      },
    },
    withContext: true,
  });

  return await connection.getRecentBlockhash(commitment);
};

const processTransaction = async ({
  connection,
  transaction,
  signers,
  commitment,
  err,
}: {
  connection: Connection,
  transaction: Transaction,
  signers: Array<Account>,
  commitment: Commitment,
  err?: any,
}) => {
  const blockhash = (await recentBlockhash({connection})).blockhash;
  transaction.recentBlockhash = blockhash;
  transaction.sign(...signers);

  const encoded = transaction.serialize().toString('base64');
  const signature = bs58.encode(transaction.signature);
  await mockRpcResponse({
    method: 'sendTransaction',
    params: [encoded],
    value: signature,
  });

  let sendOptions;
  if (err) {
    sendOptions = {
      skipPreflight: true,
    };
  } else {
    sendOptions = {
      preflightCommitment: commitment,
    };
  }

  await connection.sendEncodedTransaction(encoded, sendOptions);

  await mockRpcMessage({
    method: 'signatureSubscribe',
    params: [signature, {commitment}],
    result: {err: err || null},
  });

  return await connection.confirmTransaction(signature, commitment);
};

const airdrop = async ({
  connection,
  address,
  amount,
}: {
  connection: Connection,
  address: PublicKey,
  amount: number,
}) => {
  await mockRpcResponse({
    method: 'requestAirdrop',
    params: [address.toBase58(), amount],
    value: uniqueSignature(),
  });

  const signature = await connection.requestAirdrop(address, amount);

  await mockRpcMessage({
    method: 'signatureSubscribe',
    params: [signature, {commitment: 'confirmed'}],
    result: {err: null},
  });

  await connection.confirmTransaction(signature, 'confirmed');
  return signature;
};

export const helpers = {
  airdrop,
  processTransaction,
  recentBlockhash,
};
