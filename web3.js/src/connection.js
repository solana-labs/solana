// @flow

import assert from 'assert';
import {parse as urlParse, format as urlFormat} from 'url';
import fetch from 'node-fetch';
import jayson from 'jayson/lib/client/browser';
import {struct} from 'superstruct';
import {Client as RpcWebSocketClient} from 'rpc-websockets';

import {Transaction} from './transaction';
import {PublicKey} from './publickey';
import {sleep} from './util/sleep';
import type {Account} from './account';
import type {TransactionSignature, TransactionId} from './transaction';

type RpcRequest = (methodName: string, args: Array<any>) => any;

function createRpcRequest(url): RpcRequest {
  const server = jayson(async (request, callback) => {
    const options = {
      method: 'POST',
      body: request,
      headers: {
        'Content-Type': 'application/json',
      },
    };

    try {
      const res = await fetch(url, options);
      const text = await res.text();
      callback(null, text);
    } catch (err) {
      callback(err);
    }
  });

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
      error: 'any',
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
 * @private
 */
const AccountInfoResult = struct({
  executable: 'boolean',
  owner: 'array',
  tokens: 'number',
  userdata: 'array',
});

/**
 * Expected JSON RPC response for the "getAccountInfo" message
 */
const GetAccountInfoRpcResult = jsonRpcResult(AccountInfoResult);

/***
 * Expected JSON RPC response for the "accountNotification" message
 */
const AccountNotificationResult = struct({
  subscription: 'number',
  result: AccountInfoResult,
});

/**
 * Expected JSON RPC response for the "confirmTransaction" message
 */
const ConfirmTransactionRpcResult = jsonRpcResult('boolean');

/**
 * Expected JSON RPC response for the "getSignatureStatus" message
 */
const GetSignatureStatusRpcResult = jsonRpcResult(
  struct.enum([
    'AccountInUse',
    'Confirmed',
    'GenericFailure',
    'ProgramRuntimeError',
    'SignatureNotFound',
  ]),
);

/**
 * Expected JSON RPC response for the "getTransactionCount" message
 */
const GetTransactionCountRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getLastId" message
 */
const GetLastId = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "getConfirmationTime" message
 */
const GetConfirmationTimeRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "requestAirdrop" message
 */
const RequestAirdropRpcResult = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "sendTransaction" message
 */
const SendTransactionRpcResult = jsonRpcResult('string');

/**
 * Information describing an account
 *
 * @typedef {Object} AccountInfo
 * @property {number} tokens Number of tokens assigned to the account
 * @property {PublicKey} owner Identifier of the program that owns the account
 * @property {?Buffer} userdata Optional userdata assigned to the account
 * @property {boolean} executable `true` if this account's userdata contains a loaded program
 */
type AccountInfo = {
  executable: boolean,
  owner: PublicKey,
  tokens: number,
  userdata: Buffer,
};

/**
 * Callback function for account change notifications
 */
export type AccountChangeCallback = (accountInfo: AccountInfo) => void;

/**
 * @private
 */
type AccountSubscriptionInfo = {
  publicKey: string, // PublicKey of the account as a base 58 string
  callback: AccountChangeCallback,
  subscriptionId: null | number, // null when there's no current server subscription id
};

/**
 * Possible signature status values
 *
 * @typedef {string} SignatureStatus
 */
export type SignatureStatus =
  | 'Confirmed'
  | 'AccountInUse'
  | 'SignatureNotFound'
  | 'ProgramRuntimeError'
  | 'GenericFailure';

/**
 * A connection to a fullnode JSON RPC endpoint
 */
export class Connection {
  _rpcRequest: RpcRequest;
  _rpcWebSocket: RpcWebSocketClient;
  _rpcWebSocketConnected: boolean = false;

  _lastIdInfo: {
    lastId: TransactionId | null,
    seconds: number,
    transactionSignatures: Array<string>,
  };
  _disableLastIdCaching: boolean = false;
  _accountChangeSubscriptions: {[number]: AccountSubscriptionInfo} = {};
  _accountChangeSubscriptionCounter: number = 0;

  /**
   * Establish a JSON RPC connection
   *
   * @param endpoint URL to the fullnode JSON RPC endpoint
   */
  constructor(endpoint: string) {
    let url = urlParse(endpoint);

    this._rpcRequest = createRpcRequest(url.href);
    this._lastIdInfo = {
      lastId: null,
      seconds: -1,
      transactionSignatures: [],
    };

    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    url.host = '';
    url.port = String(Number(url.port) + 1);
    if (url.port === '1') {
      url.port = url.protocol === 'wss:' ? '8901' : '8900';
    }
    this._rpcWebSocket = new RpcWebSocketClient(urlFormat(url), {
      autoconnect: false,
      max_reconnects: Infinity,
    });
    this._rpcWebSocket.on('open', this._wsOnOpen.bind(this));
    this._rpcWebSocket.on('error', this._wsOnError.bind(this));
    this._rpcWebSocket.on('close', this._wsOnClose.bind(this));
    this._rpcWebSocket.on(
      'accountNotification',
      this._wsOnAccountNotification.bind(this),
    );
  }

  /**
   * Fetch the balance for the specified public key
   */
  async getBalance(publicKey: PublicKey): Promise<number> {
    const unsafeRes = await this._rpcRequest('getBalance', [
      publicKey.toBase58(),
    ]);
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
    const unsafeRes = await this._rpcRequest('getAccountInfo', [
      publicKey.toBase58(),
    ]);
    const res = GetAccountInfoRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }

    const {result} = res;
    assert(typeof result !== 'undefined');

    return {
      executable: result.executable,
      owner: new PublicKey(result.owner),
      tokens: result.tokens,
      userdata: Buffer.from(result.userdata),
    };
  }

  /**
   * Confirm the transaction identified by the specified signature
   */
  async confirmTransaction(signature: TransactionSignature): Promise<boolean> {
    const unsafeRes = await this._rpcRequest('confirmTransaction', [signature]);
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
  async getSignatureStatus(
    signature: TransactionSignature,
  ): Promise<SignatureStatus> {
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
   * Return the current cluster confirmation time in millliseconds
   */
  async getConfirmationTime(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getConfirmationTime', []);
    const res = GetConfirmationTimeRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Request an allocation of tokens to the specified account
   */
  async requestAirdrop(
    to: PublicKey,
    amount: number,
  ): Promise<TransactionSignature> {
    const unsafeRes = await this._rpcRequest('requestAirdrop', [
      to.toBase58(),
      amount,
    ]);
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
  async sendTransaction(
    transaction: Transaction,
    ...signers: Array<Account>
  ): Promise<TransactionSignature> {
    for (;;) {
      // Attempt to use the previous last id for up to 1 second
      const seconds = new Date().getSeconds();
      if (
        this._lastIdInfo.lastId != null &&
        this._lastIdInfo.seconds === seconds
      ) {
        transaction.lastId = this._lastIdInfo.lastId;
        transaction.sign(...signers);
        if (!transaction.signature) {
          throw new Error('!signature'); // should never happen
        }

        // If the signature of this transaction has not been seen before with the
        // current lastId, all done.
        const signature = transaction.signature.toString();
        if (!this._lastIdInfo.transactionSignatures.includes(signature)) {
          this._lastIdInfo.transactionSignatures.push(signature);
          if (this._disableLastIdCaching) {
            this._lastIdInfo.seconds = -1;
          }
          break;
        }
      }

      // Fetch a new last id
      let attempts = 0;
      const startTime = Date.now();
      for (;;) {
        const lastId = await this.getLastId();

        if (this._lastIdInfo.lastId != lastId) {
          this._lastIdInfo = {
            lastId,
            seconds: new Date().getSeconds(),
            transactionSignatures: [],
          };
          break;
        }
        if (attempts === 8) {
          throw new Error(
            `Unable to obtain a new last id after ${Date.now() - startTime}ms`,
          );
        }
        await sleep(250);
        ++attempts;
      }
    }

    const wireTransaction = transaction.serialize();
    return await this.sendRawTransaction(wireTransaction);
  }

  /**
   * Send a transaction that has already been signed and serialized into the
   * wire format
   */
  async sendRawTransaction(
    rawTransaction: Buffer,
  ): Promise<TransactionSignature> {
    // sendTransaction RPC API requires a u64 length field prepended to the raw
    // Transaction bytes
    const rpcTransaction = Buffer.alloc(8 + rawTransaction.length);
    rpcTransaction.writeUInt32LE(rawTransaction.length, 0);
    rawTransaction.copy(rpcTransaction, 8);

    const unsafeRes = await this._rpcRequest('sendTransaction', [
      [...rpcTransaction],
    ]);
    const res = SendTransactionRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    assert(res.result);
    return res.result;
  }

  /**
   * @private
   */
  _wsOnOpen() {
    this._rpcWebSocketConnected = true;
    this._updateSubscriptions();
  }

  /**
   * @private
   */
  _wsOnError(err: Error) {
    console.log('ws error:', err.message);
  }

  /**
   * @private
   */
  _wsOnClose(code: number, message: string) {
    // 1000 means _rpcWebSocket.close() was called explicitly
    if (code !== 1000) {
      console.log('ws close:', code, message);
    }
    this._rpcWebSocketConnected = false;
  }

  /**
   * @private
   */
  _wsOnAccountNotification(notification: Object) {
    const res = AccountNotificationResult(notification);
    if (res.error) {
      throw new Error(res.error.message);
    }

    const keys = Object.keys(this._accountChangeSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._accountChangeSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        const {result} = res;
        assert(typeof result !== 'undefined');

        sub.callback({
          executable: result.executable,
          owner: new PublicKey(result.owner),
          tokens: result.tokens,
          userdata: Buffer.from(result.userdata),
        });
        return true;
      }
    }
  }

  /**
   * @private
   */
  async _updateSubscriptions() {
    const keys = Object.keys(this._accountChangeSubscriptions).map(Number);
    if (keys.length === 0) {
      this._rpcWebSocket.close();
      return;
    }

    if (!this._rpcWebSocketConnected) {
      for (let id of keys) {
        this._accountChangeSubscriptions[id].subscriptionId = null;
      }
      this._rpcWebSocket.connect();
      return;
    }

    for (let id of keys) {
      const {subscriptionId, publicKey} = this._accountChangeSubscriptions[id];
      if (subscriptionId === null) {
        try {
          this._accountChangeSubscriptions[
            id
          ].subscriptionId = await this._rpcWebSocket.call('accountSubscribe', [
            publicKey,
          ]);
        } catch (err) {
          console.log(
            `accountSubscribe error for ${publicKey}: ${err.message}`,
          );
        }
      }
    }
  }

  /**
   * Register a callback to be invoked whenever the specified account changes
   *
   * @param publickey Public key of the account to monitor
   * @param callback Function to invoke whenever the account is changed
   * @return subscription id
   */
  onAccountChange(
    publicKey: PublicKey,
    callback: AccountChangeCallback,
  ): number {
    const id = ++this._accountChangeSubscriptionCounter;
    this._accountChangeSubscriptions[id] = {
      publicKey: publicKey.toBase58(),
      callback,
      subscriptionId: null,
    };
    this._updateSubscriptions();
    return id;
  }

  /**
   * Deregister an account notification callback
   *
   * @param id subscription id to deregister
   */
  async removeAccountChangeListener(id: number): Promise<void> {
    if (this._accountChangeSubscriptions[id]) {
      const {subscriptionId} = this._accountChangeSubscriptions[id];
      delete this._accountChangeSubscriptions[id];
      if (subscriptionId !== null) {
        try {
          await this._rpcWebSocket.call('accountUnsubscribe', [subscriptionId]);
        } catch (err) {
          console.log('accountUnsubscribe error:', err.message);
        }
      }
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown account change id: ${id}`);
    }
  }
}
