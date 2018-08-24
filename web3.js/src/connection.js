// @flow

import assert from 'assert';
import fetch from 'node-fetch';
import jayson from 'jayson/lib/client/browser';
import nacl from 'tweetnacl';
import {struct} from 'superstruct';

import type {Account, PublicKey} from './account';

/**
 * @typedef {string} TransactionSignature
 */
export type TransactionSignature = string;

/**
 * @typedef {string} TransactionId
 */
export type TransactionId = string;

type RpcRequest = (methodName: string, args: Array<string|number>) => any;

function createRpcRequest(url): RpcRequest {
  const server = jayson(
    async (request, callback) => {
      const options = {
        method: 'POST',
        body: request,
        headers: {
          'Content-Type': 'application/json',
        }
      };

      try {
        const res = await fetch(url, options);
        const text = await res.text();
        callback(null, text);
      } catch (err) {
        callback(err);
      }
    }
  );

  return (method, args) => {
    return new Promise((resolve, reject) => {
      server.request(method, args, (err, response) => {
        if (err) {
          reject(err);
          return;
        }
        resolve(response);
      });
    });
  };
}

const GetBalanceRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'number?',
});

const ConfirmTransactionRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'boolean?',
});

const GetTransactionCountRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'number?',
});

const GetLastId = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'string?',
});

const GetFinalityRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'number?',
});
const RequestAirdropRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'boolean?',
});

function sleep(duration = 0) {
  return new Promise((accept) => {
    setTimeout(accept, duration);
  });
}

export class Connection {
  _rpcRequest: RpcRequest;

  constructor(endpoint: string) {
    if (typeof endpoint !== 'string') {
      throw new Error('Connection endpoint not specified');
    }
    this._rpcRequest = createRpcRequest(endpoint);
  }

  async getBalance(publicKey: string): Promise<number> {
    const unsafeRes = await this._rpcRequest(
      'getBalance',
      [publicKey]
    );
    const res = GetBalanceRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  async confirmTransaction(signature: TransactionSignature): Promise<boolean> {
    const unsafeRes = await this._rpcRequest(
      'confirmTransaction',
      [signature]
    );
    const res = ConfirmTransactionRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  async getTransactionCount(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getTransactionCount', []);
    const res = GetTransactionCountRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  async getLastId(): Promise<TransactionId> {
    const unsafeRes = await this._rpcRequest('getLastId', []);
    const res = GetLastId(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  async getFinality(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getFinality', []);
    const res = GetFinalityRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  async requestAirdrop(to: PublicKey, amount: number): Promise<void> {
    const unsafeRes = await this._rpcRequest('requestAirdrop', [to, amount]);
    const res = RequestAirdropRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    assert(res.result);
  }

  async sendTokens(from: Account, to: PublicKey, amount: number): Promise<TransactionSignature> {
    const transaction = Buffer.from(
      // TODO: This is not the correct transaction payload
      `Transaction ${from.publicKey} ${to} ${amount}`
    );
    const signature = nacl.sign.detached(transaction, from.secretKey);

    console.log('Created signature of length', signature.length);
    await sleep(500); // TODO: transmit transaction + signature
    throw new Error(`Unable to send ${amount} tokens to ${to}`);
  }
}

