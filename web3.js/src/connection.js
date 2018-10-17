// @flow

import assert from 'assert';
import fetch from 'node-fetch';
import jayson from 'jayson/lib/client/browser';
import {struct} from 'superstruct';

import {Transaction} from './transaction';
import {PublicKey} from './publickey';
import type {Account} from './account';
import type {TransactionSignature, TransactionId} from './transaction';

type RpcRequest = (methodName: string, args: Array<any>) => any;

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


/**
 * Expected JSON RPC response for the "getBalance" message
 */
const GetBalanceRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: 'number?',
});


/**
 * @private
 */
function jsonRpcResult(resultDescription: any) {
  const jsonRpcVersion = struct.literal('2.0');
  return struct.union([
    struct({
      jsonrpc: jsonRpcVersion,
      id: 'string',
      error: 'any'
    }),
    struct({
      jsonrpc: jsonRpcVersion,
      id: 'string',
      error: 'null?',
      result: resultDescription,
    }),
  ]);
}


/**
 * Expected JSON RPC response for the "getAccountInfo" message
 */
const GetAccountInfoRpcResult = jsonRpcResult({
  executable: 'boolean',
  loader_program_id: 'array',
  program_id: 'array',
  tokens: 'number',
  userdata: 'array',
});


/**
 * Expected JSON RPC response for the "confirmTransaction" message
 */
const ConfirmTransactionRpcResult = jsonRpcResult('boolean');

/**
 * Expected JSON RPC response for the "getSignatureStatus" message
 */
const GetSignatureStatusRpcResult = jsonRpcResult(struct.enum([
  'Confirmed',
  'SignatureNotFound',
  'ProgramRuntimeError',
  'GenericFailure',
]));

/**
 * Expected JSON RPC response for the "getTransactionCount" message
 */
const GetTransactionCountRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getLastId" message
 */
const GetLastId = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "getFinality" message
 */
const GetFinalityRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "requestAirdrop" message
 */
const RequestAirdropRpcResult = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "sendTransaction" message
 */
const SendTokensRpcResult = jsonRpcResult('string');

/**
 * Information describing an account
 *
 * @typedef {Object} AccountInfo
 * @property {number} tokens Number of tokens assigned to the account
 * @property {PublicKey} programId Identifier of the program assigned to the account
 * @property {?Buffer} userdata Optional userdata assigned to the account
 */
type AccountInfo = {
  executable: boolean;
  programId: PublicKey,
  tokens: number,
  userdata: Buffer,
}

/**
 * Possible signature status values
 *
 * @typedef {string} SignatureStatus
 */
export type SignatureStatus = 'Confirmed' | 'SignatureNotFound' | 'ProgramRuntimeError' | 'GenericFailure';

/**
 * A connection to a fullnode JSON RPC endpoint
 */
export class Connection {
  _rpcRequest: RpcRequest;

  /**
   * Establish a JSON RPC connection
   *
   * @param endpoint URL to the fullnode JSON RPC endpoint
   */
  constructor(endpoint: string) {
    if (typeof endpoint !== 'string') {
      throw new Error('Connection endpoint not specified');
    }
    this._rpcRequest = createRpcRequest(endpoint);
  }

  /**
   * Fetch the balance for the specified public key
   */
  async getBalance(publicKey: PublicKey): Promise<number> {
    const unsafeRes = await this._rpcRequest(
      'getBalance',
      [publicKey.toBase58()]
    );
    const res = GetBalanceRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch all the account info for the specified public key
   */
  async getAccountInfo(publicKey: PublicKey): Promise<AccountInfo> {
    const unsafeRes = await this._rpcRequest(
      'getAccountInfo',
      [publicKey.toBase58()]
    );
    const res = GetAccountInfoRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }

    const {result} = res;
    assert(typeof result !== 'undefined');

    return {
      executable: result.executable,
      tokens: result.tokens,
      programId: new PublicKey(result.program_id),
      loaderProgramId: new PublicKey(result.loader_program_id),
      userdata: Buffer.from(result.userdata),
    };
  }

  /**
   * Confirm the transaction identified by the specified signature
   */
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

  /**
   * Fetch the current transaction count of the network
   */
  async getSignatureStatus(signature: TransactionSignature): Promise<SignatureStatus> {
    const unsafeRes = await this._rpcRequest('getSignatureStatus', [signature]);
    const res = GetSignatureStatusRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }


  /**
   * Fetch the current transaction count of the network
   */
  async getTransactionCount(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getTransactionCount', []);
    const res = GetTransactionCountRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Fetch the identifier to the latest transaction on the network
   */
  async getLastId(): Promise<TransactionId> {
    const unsafeRes = await this._rpcRequest('getLastId', []);
    const res = GetLastId(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Return the current network finality time in millliseconds
   */
  async getFinality(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getFinality', []);
    const res = GetFinalityRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Request an allocation of tokens to the specified account
   */
  async requestAirdrop(to: PublicKey, amount: number): Promise<TransactionSignature> {
    const unsafeRes = await this._rpcRequest('requestAirdrop', [to.toBase58(), amount]);
    const res = RequestAirdropRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Sign and send a transaction
   */
  async sendTransaction(from: Account, transaction: Transaction): Promise<TransactionSignature> {
    transaction.lastId = await this.getLastId();
    transaction.sign(from);

    const wireTransaction = transaction.serialize();
    const unsafeRes = await this._rpcRequest('sendTransaction', [[...wireTransaction]]);
    const res = SendTokensRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    assert(res.result);
    return res.result;
  }
}
