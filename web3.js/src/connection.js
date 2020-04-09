// @flow

import assert from 'assert';
import bs58 from 'bs58';
import {parse as urlParse, format as urlFormat} from 'url';
import fetch from 'node-fetch';
import jayson from 'jayson/lib/client/browser';
import {struct} from 'superstruct';
import {Client as RpcWebSocketClient} from 'rpc-websockets';

import {NonceAccount} from './nonce-account';
import {PublicKey} from './publickey';
import {DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND} from './timing';
import {Transaction} from './transaction';
import {sleep} from './util/sleep';
import {toBuffer} from './util/to-buffer';
import type {Blockhash} from './blockhash';
import type {FeeCalculator} from './fee-calculator';
import type {Account} from './account';
import type {TransactionSignature} from './transaction';

type RpcRequest = (methodName: string, args: Array<any>) => any;

/**
 * Extra contextual information for RPC responses
 *
 * @typedef {Object} Context
 * @property {number} slot
 */
type Context = {
  slot: number,
};

/**
 * RPC Response with extra contextual information
 *
 * @typedef {Object} RpcResponseAndContext
 * @property {Context} context
 * @property {T} value response
 */
type RpcResponseAndContext<T> = {
  context: Context,
  value: T,
};

/**
 * @private
 */
function jsonRpcResultAndContext(resultDescription: any) {
  return jsonRpcResult({
    context: struct({
      slot: 'number',
    }),
    value: resultDescription,
  });
}

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
function notificationResultAndContext(resultDescription: any) {
  return struct({
    context: struct({
      slot: 'number',
    }),
    value: resultDescription,
  });
}

/**
 * The level of commitment desired when querying state
 *   'max':    Query the most recent block which has reached max voter lockout
 *   'recent': Query the most recent block
 *
 * @typedef {'max' | 'recent'} Commitment
 */
export type Commitment = 'max' | 'recent';

/**
 * Configuration object for changing query behavior
 *
 * @typedef {Object} SignatureStatusConfig
 * @property {boolean} searchTransactionHistory enable searching status history, not needed for recent transactions
 */
export type SignatureStatusConfig = {
  searchTransactionHistory: boolean,
};

/**
 * Information describing a cluster node
 *
 * @typedef {Object} ContactInfo
 * @property {string} pubkey Identity public key of the node
 * @property {string} gossip Gossip network address for the node
 * @property {string} tpu TPU network address for the node (null if not available)
 * @property {string|null} rpc JSON RPC network address for the node (null if not available)
 */
type ContactInfo = {
  pubkey: string,
  gossip: string,
  tpu: string | null,
  rpc: string | null,
};

/**
 * Information describing a vote account
 *
 * @typedef {Object} VoteAccountInfo
 * @property {string} votePubkey Public key of the vote account
 * @property {string} nodePubkey Identity public key of the node voting with this account
 * @property {number} activatedStake The stake, in lamports, delegated to this vote account and activated
 * @property {boolean} epochVoteAccount Whether the vote account is staked for this epoch
 * @property {Array<Array<number>>} epochCredits Recent epoch voting credit history for this voter
 * @property {number} commission A percentage (0-100) of rewards payout owed to the voter
 * @property {number} lastVote Most recent slot voted on by this vote account
 */
type VoteAccountInfo = {
  votePubkey: string,
  nodePubkey: string,
  activatedStake: number,
  epochVoteAccount: boolean,
  epochCredits: Array<[number, number, number]>,
  commission: number,
  lastVote: number,
};

/**
 * A collection of cluster vote accounts
 *
 * @typedef {Object} VoteAccountStatus
 * @property {Array<VoteAccountInfo>} current Active vote accounts
 * @property {Array<VoteAccountInfo>} delinquent Inactive vote accounts
 */
type VoteAccountStatus = {
  current: Array<VoteAccountInfo>,
  delinquent: Array<VoteAccountInfo>,
};

/**
 * Network Inflation parameters
 * (see https://docs.solana.com/implemented-proposals/ed_overview)
 *
 * @typedef {Object} Inflation
 * @property {number} foundation
 * @property {number} foundation_term
 * @property {number} initial
 * @property {number} storage
 * @property {number} taper
 * @property {number} terminal
 */
const GetInflationResult = struct({
  foundation: 'number',
  foundationTerm: 'number',
  initial: 'number',
  storage: 'number',
  taper: 'number',
  terminal: 'number',
});

/**
 * EpochInfo parameters
 * (see https://docs.solana.com/terminology#epoch)
 *
 * @typedef {Object} EpochInfo
 * @property {number} epoch
 * @property {number} slotIndex
 * @property {number} slotsInEpoch
 * @property {number} absoluteSlot
 */
const GetEpochInfoResult = struct({
  epoch: 'number',
  slotIndex: 'number',
  slotsInEpoch: 'number',
  absoluteSlot: 'number',
});

/**
 * EpochSchedule parameters
 * (see https://docs.solana.com/terminology#epoch)
 *
 * @typedef {Object} EpochSchedule
 * @property {number} slotsPerEpoch The maximum number of slots in each epoch
 * @property {number} leaderScheduleSlotOffset The number of slots before beginning of an epoch to calculate a leader schedule for that epoch
 * @property {boolean} warmup Indicates whether epochs start short and grow
 * @property {number} firstNormalEpoch The first epoch with `slotsPerEpoch` slots
 * @property {number} firstNormalSlot The first slot of `firstNormalEpoch`
 */
const GetEpochScheduleResult = struct({
  slotsPerEpoch: 'number',
  leaderScheduleSlotOffset: 'number',
  warmup: 'boolean',
  firstNormalEpoch: 'number',
  firstNormalSlot: 'number',
});

/**
 * Transaction error or null
 */
const TransactionErrorResult = struct.union(['null', 'object']);

/**
 * Signature status for a transaction
 */
const SignatureStatusResult = struct({err: TransactionErrorResult});

/**
 * Version info for a node
 *
 * @typedef {Object} Version
 * @property {string} solana-core Version of solana-core
 */
const Version = struct({
  'solana-core': 'string',
});

/**
 * A ConfirmedBlock on the ledger
 *
 * @typedef {Object} ConfirmedBlock
 * @property {Blockhash} blockhash Blockhash of this block
 * @property {Blockhash} previousBlockhash Blockhash of this block's parent
 * @property {number} parentSlot Slot index of this block's parent
 * @property {Array<object>} transactions Vector of transactions and status metas
 * @property {Array<object>} rewards Vector of block rewards
 */
type ConfirmedBlock = {
  blockhash: Blockhash,
  previousBlockhash: Blockhash,
  parentSlot: number,
  transactions: Array<{
    transaction: Transaction,
    meta: {
      fee: number,
      preBalances: Array<number>,
      postBalances: Array<number>,
      err: TransactionError | null,
    } | null,
  }>,
  rewards: Array<{
    pubkey: string,
    lamports: number,
  }>,
};

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
 * Expected JSON RPC response for the "getInflation" message
 */
const GetInflationRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: GetInflationResult,
});

/**
 * Expected JSON RPC response for the "getEpochInfo" message
 */
const GetEpochInfoRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: GetEpochInfoResult,
});

/**
 * Expected JSON RPC response for the "getEpochSchedule" message
 */
const GetEpochScheduleRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: GetEpochScheduleResult,
});

/**
 * Expected JSON RPC response for the "getBalance" message
 */
const GetBalanceAndContextRpcResult = jsonRpcResultAndContext('number?');

/**
 * Expected JSON RPC response for the "getVersion" message
 */
const GetVersionRpcResult = struct({
  jsonrpc: struct.literal('2.0'),
  id: 'string',
  error: 'any?',
  result: Version,
});

/**
 * @private
 */
const AccountInfoResult = struct({
  executable: 'boolean',
  owner: 'string',
  lamports: 'number',
  data: 'string',
  rentEpoch: 'number?',
});

/**
 * Expected JSON RPC response for the "getAccountInfo" message
 */
const GetAccountInfoAndContextRpcResult = jsonRpcResultAndContext(
  struct.union(['null', AccountInfoResult]),
);

/***
 * Expected JSON RPC response for the "accountNotification" message
 */
const AccountNotificationResult = struct({
  subscription: 'number',
  result: notificationResultAndContext(AccountInfoResult),
});

/**
 * @private
 */
const ProgramAccountInfoResult = struct({
  pubkey: 'string',
  account: AccountInfoResult,
});

/***
 * Expected JSON RPC response for the "programNotification" message
 */
const ProgramAccountNotificationResult = struct({
  subscription: 'number',
  result: notificationResultAndContext(ProgramAccountInfoResult),
});

/**
 * @private
 */
const SlotInfoResult = struct({
  parent: 'number',
  slot: 'number',
  root: 'number',
});

/**
 * Expected JSON RPC response for the "slotNotification" message
 */
const SlotNotificationResult = struct({
  subscription: 'number',
  result: SlotInfoResult,
});

/**
 * Expected JSON RPC response for the "signatureNotification" message
 */
const SignatureNotificationResult = struct({
  subscription: 'number',
  result: notificationResultAndContext(SignatureStatusResult),
});

/**
 * Expected JSON RPC response for the "rootNotification" message
 */
const RootNotificationResult = struct({
  subscription: 'number',
  result: 'number',
});

/**
 * Expected JSON RPC response for the "getProgramAccounts" message
 */
const GetProgramAccountsRpcResult = jsonRpcResult(
  struct.array([ProgramAccountInfoResult]),
);

/**
 * Expected JSON RPC response for the "confirmTransaction" message
 */
const ConfirmTransactionAndContextRpcResult = jsonRpcResultAndContext(
  'boolean',
);

/**
 * Expected JSON RPC response for the "getSlot" message
 */
const GetSlot = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getSlotLeader" message
 */
const GetSlotLeader = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "getClusterNodes" message
 */
const GetClusterNodes = jsonRpcResult(
  struct.array([
    struct({
      pubkey: 'string',
      gossip: 'string',
      tpu: struct.union(['null', 'string']),
      rpc: struct.union(['null', 'string']),
    }),
  ]),
);

/**
 * Expected JSON RPC response for the "getVoteAccounts" message
 */
const GetVoteAccounts = jsonRpcResult(
  struct({
    current: struct.array([
      struct({
        votePubkey: 'string',
        nodePubkey: 'string',
        activatedStake: 'number',
        epochVoteAccount: 'boolean',
        epochCredits: struct.array([
          struct.tuple(['number', 'number', 'number']),
        ]),
        commission: 'number',
        lastVote: 'number',
        rootSlot: 'number?',
      }),
    ]),
    delinquent: struct.array([
      struct({
        votePubkey: 'string',
        nodePubkey: 'string',
        activatedStake: 'number',
        epochVoteAccount: 'boolean',
        epochCredits: struct.array([
          struct.tuple(['number', 'number', 'number']),
        ]),
        commission: 'number',
        lastVote: 'number',
        rootSlot: 'number?',
      }),
    ]),
  }),
);

/**
 * Expected JSON RPC response for the "getSignatureStatuses" message
 */
const GetSignatureStatusesRpcResult = jsonRpcResultAndContext(
  struct.array([
    struct.union([
      'null',
      struct.pick({
        slot: 'number',
        confirmations: struct.union(['number', 'null']),
        err: TransactionErrorResult,
      }),
    ]),
  ]),
);

/**
 * Expected JSON RPC response for the "getTransactionCount" message
 */
const GetTransactionCountRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getTotalSupply" message
 */
const GetTotalSupplyRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getMinimumBalanceForRentExemption" message
 */
const GetMinimumBalanceForRentExemptionRpcResult = jsonRpcResult('number');

/**
 * Expected JSON RPC response for the "getConfirmedBlock" message
 */
export const GetConfirmedBlockRpcResult = jsonRpcResult(
  struct.union([
    'null',
    struct({
      blockhash: 'string',
      previousBlockhash: 'string',
      parentSlot: 'number',
      transactions: struct.array([
        struct({
          transaction: struct({
            signatures: struct.array(['string']),
            message: struct({
              accountKeys: struct.array(['string']),
              header: struct({
                numRequiredSignatures: 'number',
                numReadonlySignedAccounts: 'number',
                numReadonlyUnsignedAccounts: 'number',
              }),
              instructions: struct.array([
                struct.union([
                  struct.array(['number']),
                  struct({
                    accounts: struct.array(['number']),
                    data: 'string',
                    programIdIndex: 'number',
                  }),
                ]),
              ]),
              recentBlockhash: 'string',
            }),
          }),
          meta: struct.union([
            'null',
            struct.pick({
              err: TransactionErrorResult,
              fee: 'number',
              preBalances: struct.array(['number']),
              postBalances: struct.array(['number']),
            }),
          ]),
        }),
      ]),
      rewards: struct.union([
        'undefined',
        struct.array([
          struct({
            pubkey: 'string',
            lamports: 'number',
          }),
        ]),
      ]),
    }),
  ]),
);

/**
 * Expected JSON RPC response for the "getRecentBlockhash" message
 */
const GetRecentBlockhashAndContextRpcResult = jsonRpcResultAndContext(
  struct({
    blockhash: 'string',
    feeCalculator: struct({
      lamportsPerSignature: 'number',
    }),
  }),
);

/**
 * Expected JSON RPC response for the "requestAirdrop" message
 */
const RequestAirdropRpcResult = jsonRpcResult('string');

/**
 * Expected JSON RPC response for the "sendTransaction" message
 */
const SendTransactionRpcResult = jsonRpcResult('string');

/**
 * Information about the latest slot being processed by a node
 *
 * @typedef {Object} SlotInfo
 * @property {number} slot Currently processing slot
 * @property {number} parent Parent of the current slot
 * @property {number} root The root block of the current slot's fork
 */
type SlotInfo = {
  slot: number,
  parent: number,
  root: number,
};

/**
 * Information describing an account
 *
 * @typedef {Object} AccountInfo
 * @property {number} lamports Number of lamports assigned to the account
 * @property {PublicKey} owner Identifier of the program that owns the account
 * @property {?Buffer} data Optional data assigned to the account
 * @property {boolean} executable `true` if this account's data contains a loaded program
 */
type AccountInfo = {
  executable: boolean,
  owner: PublicKey,
  lamports: number,
  data: Buffer,
};

/**
 * Account information identified by pubkey
 *
 * @typedef {Object} KeyedAccountInfo
 * @property {PublicKey} accountId
 * @property {AccountInfo} accountInfo
 */
type KeyedAccountInfo = {
  accountId: PublicKey,
  accountInfo: AccountInfo,
};

/**
 * Callback function for account change notifications
 */
export type AccountChangeCallback = (
  accountInfo: AccountInfo,
  context: Context,
) => void;

/**
 * @private
 */
type SubscriptionId = 'subscribing' | number;

/**
 * @private
 */
type AccountSubscriptionInfo = {
  publicKey: string, // PublicKey of the account as a base 58 string
  callback: AccountChangeCallback,
  subscriptionId: ?SubscriptionId, // null when there's no current server subscription id
};

/**
 * Callback function for program account change notifications
 */
export type ProgramAccountChangeCallback = (
  keyedAccountInfo: KeyedAccountInfo,
  context: Context,
) => void;

/**
 * @private
 */
type ProgramAccountSubscriptionInfo = {
  programId: string, // PublicKey of the program as a base 58 string
  callback: ProgramAccountChangeCallback,
  subscriptionId: ?SubscriptionId, // null when there's no current server subscription id
};

/**
 * Callback function for slot change notifications
 */
export type SlotChangeCallback = (slotInfo: SlotInfo) => void;

/**
 * @private
 */
type SlotSubscriptionInfo = {
  callback: SlotChangeCallback,
  subscriptionId: ?SubscriptionId, // null when there's no current server subscription id
};

/**
 * Callback function for signature notifications
 */
export type SignatureResultCallback = (
  signatureResult: SignatureResult,
  context: Context,
) => void;

/**
 * @private
 */
type SignatureSubscriptionInfo = {
  signature: TransactionSignature, // TransactionSignature as a base 58 string
  callback: SignatureResultCallback,
  subscriptionId: ?SubscriptionId, // null when there's no current server subscription id
};

/**
 * Callback function for root change notifications
 */
export type RootChangeCallback = (root: number) => void;

/**
 * @private
 */
type RootSubscriptionInfo = {
  callback: RootChangeCallback,
  subscriptionId: ?SubscriptionId, // null when there's no current server subscription id
};

/**
 * Signature result
 *
 * @typedef {Object} SignatureResult
 */
export type SignatureResult = {|
  err: TransactionError | null,
|};

/**
 * Transaction error
 *
 * @typedef {Object} TransactionError
 */
export type TransactionError = {};

/**
 * Signature status
 *
 * @typedef {Object} SignatureStatus
 * @property {number} slot when the transaction was processed
 * @property {number | null} confirmations the number of blocks that have been confirmed and voted on in the fork containing `slot` (TODO)
 * @property {TransactionError | null} err error, if any
 */
export type SignatureStatus = {
  slot: number,
  confirmations: number | null,
  err: TransactionError | null,
};

/**
 * A connection to a fullnode JSON RPC endpoint
 */
export class Connection {
  _rpcRequest: RpcRequest;
  _rpcWebSocket: RpcWebSocketClient;
  _rpcWebSocketConnected: boolean = false;

  _commitment: ?Commitment;
  _blockhashInfo: {
    recentBlockhash: Blockhash | null,
    seconds: number,
    transactionSignatures: Array<string>,
  };
  _disableBlockhashCaching: boolean = false;
  _accountChangeSubscriptions: {[number]: AccountSubscriptionInfo} = {};
  _accountChangeSubscriptionCounter: number = 0;
  _programAccountChangeSubscriptions: {
    [number]: ProgramAccountSubscriptionInfo,
  } = {};
  _programAccountChangeSubscriptionCounter: number = 0;
  _slotSubscriptions: {
    [number]: SlotSubscriptionInfo,
  } = {};
  _slotSubscriptionCounter: number = 0;
  _signatureSubscriptions: {
    [number]: SignatureSubscriptionInfo,
  } = {};
  _signatureSubscriptionCounter: number = 0;
  _rootSubscriptions: {
    [number]: RootSubscriptionInfo,
  } = {};
  _rootSubscriptionCounter: number = 0;

  /**
   * Establish a JSON RPC connection
   *
   * @param endpoint URL to the fullnode JSON RPC endpoint
   * @param commitment optional default commitment level
   */
  constructor(endpoint: string, commitment: ?Commitment) {
    let url = urlParse(endpoint);

    this._rpcRequest = createRpcRequest(url.href);
    this._commitment = commitment;
    this._blockhashInfo = {
      recentBlockhash: null,
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
    this._rpcWebSocket.on(
      'programNotification',
      this._wsOnProgramAccountNotification.bind(this),
    );
    this._rpcWebSocket.on(
      'slotNotification',
      this._wsOnSlotNotification.bind(this),
    );
    this._rpcWebSocket.on(
      'signatureNotification',
      this._wsOnSignatureNotification.bind(this),
    );
    this._rpcWebSocket.on(
      'rootNotification',
      this._wsOnRootNotification.bind(this),
    );
  }

  /**
   * The default commitment used for requests
   */
  get commitment(): ?Commitment {
    return this._commitment;
  }

  /**
   * Fetch the balance for the specified public key, return with context
   */
  async getBalanceAndContext(
    publicKey: PublicKey,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<number>> {
    const args = this._argsWithCommitment([publicKey.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getBalance', args);
    const res = GetBalanceAndContextRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(
        'failed to get balance for ' +
          publicKey.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch the balance for the specified public key
   */
  async getBalance(
    publicKey: PublicKey,
    commitment: ?Commitment,
  ): Promise<number> {
    return await this.getBalanceAndContext(publicKey, commitment)
      .then(x => x.value)
      .catch(e => {
        throw new Error(
          'failed to get balance of account ' + publicKey.toBase58() + ': ' + e,
        );
      });
  }

  /**
   * Fetch all the account info for the specified public key, return with context
   */
  async getAccountInfoAndContext(
    publicKey: PublicKey,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<AccountInfo | null>> {
    const args = this._argsWithCommitment([publicKey.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getAccountInfo', args);
    const res = GetAccountInfoAndContextRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(
        'failed to get info about account ' +
          publicKey.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    assert(typeof res.result !== 'undefined');

    let value = null;
    if (res.result.value) {
      const {executable, owner, lamports, data} = res.result.value;
      value = {
        executable,
        owner: new PublicKey(owner),
        lamports,
        data: bs58.decode(data),
      };
    }

    return {
      context: {
        slot: res.result.context.slot,
      },
      value,
    };
  }

  /**
   * Fetch all the account info for the specified public key
   */
  async getAccountInfo(
    publicKey: PublicKey,
    commitment: ?Commitment,
  ): Promise<AccountInfo | null> {
    return await this.getAccountInfoAndContext(publicKey, commitment)
      .then(x => x.value)
      .catch(e => {
        throw new Error(
          'failed to get info about account ' + publicKey.toBase58() + ': ' + e,
        );
      });
  }

  /**
   * Fetch all the accounts owned by the specified program id
   *
   * @return {Promise<Array<{pubkey: PublicKey, account: AccountInfo}>>}
   */
  async getProgramAccounts(
    programId: PublicKey,
    commitment: ?Commitment,
  ): Promise<Array<{pubkey: PublicKey, account: AccountInfo}>> {
    const args = this._argsWithCommitment([programId.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getProgramAccounts', args);
    const res = GetProgramAccountsRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(
        'failed to get accounts owned by program ' +
          programId.toBase58() +
          ': ' +
          res.error.message,
      );
    }

    const {result} = res;
    assert(typeof result !== 'undefined');

    return result.map(result => {
      return {
        pubkey: result.pubkey,
        account: {
          executable: result.account.executable,
          owner: new PublicKey(result.account.owner),
          lamports: result.account.lamports,
          data: bs58.decode(result.account.data),
        },
      };
    });
  }

  /**
   * Confirm the transaction identified by the specified signature, return with context
   */
  async confirmTransactionAndContext(
    signature: TransactionSignature,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<boolean>> {
    const args = this._argsWithCommitment([signature], commitment);
    const unsafeRes = await this._rpcRequest('confirmTransaction', args);
    const res = ConfirmTransactionAndContextRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to confirm transaction: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Confirm the transaction identified by the specified signature
   */
  async confirmTransaction(
    signature: TransactionSignature,
    commitment: ?Commitment,
  ): Promise<boolean> {
    return await this.confirmTransactionAndContext(signature, commitment)
      .then(x => x.value)
      .catch(e => {
        throw new Error('failed to confirm transaction: ' + e);
      });
  }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getClusterNodes(): Promise<Array<ContactInfo>> {
    const unsafeRes = await this._rpcRequest('getClusterNodes', []);

    const res = GetClusterNodes(unsafeRes);
    if (res.error) {
      throw new Error('failed to get cluster nodes: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getVoteAccounts(commitment: ?Commitment): Promise<VoteAccountStatus> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getVoteAccounts', args);
    const res = GetVoteAccounts(unsafeRes);
    //const res = unsafeRes;
    if (res.error) {
      throw new Error('failed to get vote accounts: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch the current slot that the node is processing
   */
  async getSlot(commitment: ?Commitment): Promise<number> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getSlot', args);
    const res = GetSlot(unsafeRes);
    if (res.error) {
      throw new Error('failed to get slot: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch the current slot leader of the cluster
   */
  async getSlotLeader(commitment: ?Commitment): Promise<string> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getSlotLeader', args);
    const res = GetSlotLeader(unsafeRes);
    if (res.error) {
      throw new Error('failed to get slot leader: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch the current status of a signature
   */
  async getSignatureStatus(
    signature: TransactionSignature,
    config: ?SignatureStatusConfig,
  ): Promise<RpcResponseAndContext<SignatureStatus | null>> {
    const {context, value} = await this.getSignatureStatuses(
      [signature],
      config,
    );
    assert(value.length === 1);
    return {context, value: value[0]};
  }

  /**
   * Fetch the current statuses of a batch of signatures
   */
  async getSignatureStatuses(
    signatures: Array<TransactionSignature>,
    config: ?SignatureStatusConfig,
  ): Promise<RpcResponseAndContext<Array<SignatureStatus | null>>> {
    const params = [signatures];
    if (config) {
      params.push(config);
    }
    const unsafeRes = await this._rpcRequest('getSignatureStatuses', params);
    const res = GetSignatureStatusesRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get signature status: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch the current transaction count of the cluster
   */
  async getTransactionCount(commitment: ?Commitment): Promise<number> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getTransactionCount', args);
    const res = GetTransactionCountRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get transaction count: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Fetch the current total currency supply of the cluster in lamports
   */
  async getTotalSupply(commitment: ?Commitment): Promise<number> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getTotalSupply', args);
    const res = GetTotalSupplyRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('faied to get total supply: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Fetch the cluster Inflation parameters
   */
  async getInflation(commitment: ?Commitment): Promise<GetInflationRpcResult> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getInflation', args);
    const res = GetInflationRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get inflation: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return GetInflationResult(res.result);
  }

  /**
   * Fetch the Epoch Info parameters
   */
  async getEpochInfo(commitment: ?Commitment): Promise<GetEpochInfoRpcResult> {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getEpochInfo', args);
    const res = GetEpochInfoRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get epoch info: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return GetEpochInfoResult(res.result);
  }

  /**
   * Fetch the Epoch Schedule parameters
   */
  async getEpochSchedule(): Promise<GetEpochScheduleRpcResult> {
    const unsafeRes = await this._rpcRequest('getEpochSchedule', []);
    const res = GetEpochScheduleRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get epoch schedule: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return GetEpochScheduleResult(res.result);
  }

  /**
   * Fetch the minimum balance needed to exempt an account of `dataLength`
   * size from rent
   */
  async getMinimumBalanceForRentExemption(
    dataLength: number,
    commitment: ?Commitment,
  ): Promise<number> {
    const args = this._argsWithCommitment([dataLength], commitment);
    const unsafeRes = await this._rpcRequest(
      'getMinimumBalanceForRentExemption',
      args,
    );
    const res = GetMinimumBalanceForRentExemptionRpcResult(unsafeRes);
    if (res.error) {
      console.warn('Unable to fetch minimum balance for rent exemption');
      return 0;
    }
    assert(typeof res.result !== 'undefined');
    return Number(res.result);
  }

  /**
   * Fetch a recent blockhash from the cluster, return with context
   * @return {Promise<RpcResponseAndContext<{blockhash: Blockhash, feeCalculator: FeeCalculator}>>}
   */
  async getRecentBlockhashAndContext(
    commitment: ?Commitment,
  ): Promise<
    RpcResponseAndContext<{blockhash: Blockhash, feeCalculator: FeeCalculator}>,
  > {
    const args = this._argsWithCommitment([], commitment);
    const unsafeRes = await this._rpcRequest('getRecentBlockhash', args);

    const res = GetRecentBlockhashAndContextRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get recent blockhash: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch a recent blockhash from the cluster
   * @return {Promise<{blockhash: Blockhash, feeCalculator: FeeCalculator}>}
   */
  async getRecentBlockhash(
    commitment: ?Commitment,
  ): Promise<{blockhash: Blockhash, feeCalculator: FeeCalculator}> {
    return await this.getRecentBlockhashAndContext(commitment)
      .then(x => x.value)
      .catch(e => {
        throw new Error('failed to get recent blockhash: ' + e);
      });
  }

  /**
   * Fetch the node version
   */
  async getVersion(): Promise<Version> {
    const unsafeRes = await this._rpcRequest('getVersion', []);
    const res = GetVersionRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to get version: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Fetch a list of Transactions and transaction statuses from the cluster
   * for a confirmed block
   */
  async getConfirmedBlock(slot: number): Promise<ConfirmedBlock> {
    const unsafeRes = await this._rpcRequest('getConfirmedBlock', [slot]);
    const result = GetConfirmedBlockRpcResult(unsafeRes);
    if (result.error) {
      throw new Error('failed to get confirmed block: ' + result.error.message);
    }
    assert(typeof result.result !== 'undefined');
    if (!result.result) {
      throw new Error('Confirmed block ' + slot + ' not found');
    }
    return {
      blockhash: new PublicKey(result.result.blockhash).toString(),
      previousBlockhash: new PublicKey(
        result.result.previousBlockhash,
      ).toString(),
      parentSlot: result.result.parentSlot,
      transactions: result.result.transactions.map(result => {
        return {
          transaction: Transaction.fromRpcResult(result.transaction),
          meta: result.meta,
        };
      }),
      rewards: result.result.rewards || [],
    };
  }

  /**
   * Fetch the contents of a Nonce account from the cluster, return with context
   */
  async getNonceAndContext(
    nonceAccount: PublicKey,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<NonceAccount | null>> {
    const {context, value: accountInfo} = await this.getAccountInfoAndContext(
      nonceAccount,
      commitment,
    );

    let value = null;
    if (accountInfo !== null) {
      value = NonceAccount.fromAccountData(accountInfo.data);
    }

    return {
      context,
      value,
    };
  }

  /**
   * Fetch the contents of a Nonce account from the cluster
   */
  async getNonce(
    nonceAccount: PublicKey,
    commitment: ?Commitment,
  ): Promise<NonceAccount | null> {
    return await this.getNonceAndContext(nonceAccount, commitment)
      .then(x => x.value)
      .catch(e => {
        throw new Error(
          'failed to get nonce for account ' +
            nonceAccount.toBase58() +
            ': ' +
            e,
        );
      });
  }

  /**
   * Request an allocation of lamports to the specified account
   */
  async requestAirdrop(
    to: PublicKey,
    amount: number,
    commitment: ?Commitment,
  ): Promise<TransactionSignature> {
    const args = this._argsWithCommitment([to.toBase58(), amount], commitment);
    const unsafeRes = await this._rpcRequest('requestAirdrop', args);
    const res = RequestAirdropRpcResult(unsafeRes);
    if (res.error) {
      throw new Error(
        'airdrop to ' + to.toBase58() + ' failed: ' + res.error.message,
      );
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
    if (transaction.nonceInfo) {
      transaction.sign(...signers);
    } else {
      for (;;) {
        // Attempt to use a recent blockhash for up to 30 seconds
        const seconds = new Date().getSeconds();
        if (
          this._blockhashInfo.recentBlockhash != null &&
          this._blockhashInfo.seconds < seconds + 30
        ) {
          transaction.recentBlockhash = this._blockhashInfo.recentBlockhash;
          transaction.sign(...signers);
          if (!transaction.signature) {
            throw new Error('!signature'); // should never happen
          }

          // If the signature of this transaction has not been seen before with the
          // current recentBlockhash, all done.
          const signature = transaction.signature.toString();
          if (!this._blockhashInfo.transactionSignatures.includes(signature)) {
            this._blockhashInfo.transactionSignatures.push(signature);
            if (this._disableBlockhashCaching) {
              this._blockhashInfo.seconds = -1;
            }
            break;
          }
        }

        // Fetch a new blockhash
        let attempts = 0;
        const startTime = Date.now();
        for (;;) {
          const {blockhash} = await this.getRecentBlockhash();

          if (this._blockhashInfo.recentBlockhash != blockhash) {
            this._blockhashInfo = {
              recentBlockhash: blockhash,
              seconds: new Date().getSeconds(),
              transactionSignatures: [],
            };
            break;
          }
          if (attempts === 50) {
            throw new Error(
              `Unable to obtain a new blockhash after ${
                Date.now() - startTime
              }ms`,
            );
          }

          // Sleep for approximately half a slot
          await sleep((500 * DEFAULT_TICKS_PER_SLOT) / NUM_TICKS_PER_SECOND);

          ++attempts;
        }
      }
    }

    const wireTransaction = transaction.serialize();
    return await this.sendRawTransaction(wireTransaction);
  }

  /**
   * @private
   */
  async validatorExit(): Promise<boolean> {
    const unsafeRes = await this._rpcRequest('validatorExit', []);
    const res = jsonRpcResult('boolean')(unsafeRes);
    if (res.error) {
      throw new Error('validator exit failed: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    return res.result;
  }

  /**
   * Send a transaction that has already been signed and serialized into the
   * wire format
   */
  async sendRawTransaction(
    rawTransaction: Buffer | Uint8Array | Array<number>,
  ): Promise<TransactionSignature> {
    const encodedTransaction = bs58.encode(toBuffer(rawTransaction));
    const result = await this.sendEncodedTransaction(encodedTransaction);
    return result;
  }

  /**
   * Send a transaction that has already been signed, serialized into the
   * wire format, and encoded as a base58 string
   */
  async sendEncodedTransaction(
    encodedTransaction: string,
  ): Promise<TransactionSignature> {
    const unsafeRes = await this._rpcRequest('sendTransaction', [
      encodedTransaction,
    ]);
    const res = SendTransactionRpcResult(unsafeRes);
    if (res.error) {
      throw new Error('failed to send transaction: ' + res.error.message);
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
    console.error('ws error:', err.message);
  }

  /**
   * @private
   */
  _wsOnClose() {
    this._rpcWebSocketConnected = false;
    this._resetSubscriptions();
  }

  /**
   * @private
   */
  async _subscribe<SubInfo: {subscriptionId: ?SubscriptionId}, RpcArgs>(
    sub: SubInfo,
    rpcMethod: string,
    rpcArgs: RpcArgs,
  ) {
    if (sub.subscriptionId == null) {
      sub.subscriptionId = 'subscribing';
      try {
        const id = await this._rpcWebSocket.call(rpcMethod, rpcArgs);
        if (sub.subscriptionId === 'subscribing') {
          // eslint-disable-next-line require-atomic-updates
          sub.subscriptionId = id;
        }
      } catch (err) {
        if (sub.subscriptionId === 'subscribing') {
          // eslint-disable-next-line require-atomic-updates
          sub.subscriptionId = null;
        }
        console.error(`${rpcMethod} error for argument`, rpcArgs, err.message);
      }
    }
  }

  /**
   * @private
   */
  async _unsubscribe<SubInfo: {subscriptionId: ?SubscriptionId}>(
    sub: SubInfo,
    rpcMethod: string,
  ) {
    const subscriptionId = sub.subscriptionId;
    if (subscriptionId != null && typeof subscriptionId != 'string') {
      const unsubscribeId: number = subscriptionId;
      try {
        await this._rpcWebSocket.call(rpcMethod, [unsubscribeId]);
      } catch (err) {
        console.error(`${rpcMethod} error:`, err.message);
      }
    }
  }

  /**
   * @private
   */
  _resetSubscriptions() {
    (Object.values(this._accountChangeSubscriptions): any).forEach(
      s => (s.subscriptionId = null),
    );
    (Object.values(this._programAccountChangeSubscriptions): any).forEach(
      s => (s.subscriptionId = null),
    );
    (Object.values(this._signatureSubscriptions): any).forEach(
      s => (s.subscriptionId = null),
    );
    (Object.values(this._slotSubscriptions): any).forEach(
      s => (s.subscriptionId = null),
    );
    (Object.values(this._rootSubscriptions): any).forEach(
      s => (s.subscriptionId = null),
    );
  }

  /**
   * @private
   */
  _updateSubscriptions() {
    const accountKeys = Object.keys(this._accountChangeSubscriptions).map(
      Number,
    );
    const programKeys = Object.keys(
      this._programAccountChangeSubscriptions,
    ).map(Number);
    const slotKeys = Object.keys(this._slotSubscriptions).map(Number);
    const signatureKeys = Object.keys(this._signatureSubscriptions).map(Number);
    const rootKeys = Object.keys(this._rootSubscriptions).map(Number);
    if (
      accountKeys.length === 0 &&
      programKeys.length === 0 &&
      slotKeys.length === 0 &&
      signatureKeys.length === 0 &&
      rootKeys.length === 0
    ) {
      this._rpcWebSocket.close();
      return;
    }

    if (!this._rpcWebSocketConnected) {
      this._resetSubscriptions();
      this._rpcWebSocket.connect();
      return;
    }

    for (let id of accountKeys) {
      const sub = this._accountChangeSubscriptions[id];
      this._subscribe(sub, 'accountSubscribe', [sub.publicKey]);
    }

    for (let id of programKeys) {
      const sub = this._programAccountChangeSubscriptions[id];
      this._subscribe(sub, 'programSubscribe', [sub.programId]);
    }

    for (let id of slotKeys) {
      const sub = this._slotSubscriptions[id];
      this._subscribe(sub, 'slotSubscribe', []);
    }

    for (let id of signatureKeys) {
      const sub = this._signatureSubscriptions[id];
      this._subscribe(sub, 'signatureSubscribe', [sub.signature]);
    }

    for (let id of rootKeys) {
      const sub = this._rootSubscriptions[id];
      this._subscribe(sub, 'rootSubscribe', []);
    }
  }

  /**
   * @private
   */
  _wsOnAccountNotification(notification: Object) {
    const res = AccountNotificationResult(notification);
    if (res.error) {
      throw new Error('account notification failed: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    const keys = Object.keys(this._accountChangeSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._accountChangeSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        const {result} = res;
        const {value, context} = result;

        sub.callback(
          {
            executable: value.executable,
            owner: new PublicKey(value.owner),
            lamports: value.lamports,
            data: bs58.decode(value.data),
          },
          context,
        );
        return true;
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
      const subInfo = this._accountChangeSubscriptions[id];
      delete this._accountChangeSubscriptions[id];
      await this._unsubscribe(subInfo, 'accountUnsubscribe');
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown account change id: ${id}`);
    }
  }

  /**
   * @private
   */
  _wsOnProgramAccountNotification(notification: Object) {
    const res = ProgramAccountNotificationResult(notification);
    if (res.error) {
      throw new Error(
        'program account notification failed: ' + res.error.message,
      );
    }
    assert(typeof res.result !== 'undefined');
    const keys = Object.keys(this._programAccountChangeSubscriptions).map(
      Number,
    );
    for (let id of keys) {
      const sub = this._programAccountChangeSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        const {result} = res;
        const {value, context} = result;

        sub.callback(
          {
            accountId: value.pubkey,
            accountInfo: {
              executable: value.account.executable,
              owner: new PublicKey(value.account.owner),
              lamports: value.account.lamports,
              data: bs58.decode(value.account.data),
            },
          },
          context,
        );
        return true;
      }
    }
  }

  /**
   * Register a callback to be invoked whenever accounts owned by the
   * specified program change
   *
   * @param programId Public key of the program to monitor
   * @param callback Function to invoke whenever the account is changed
   * @return subscription id
   */
  onProgramAccountChange(
    programId: PublicKey,
    callback: ProgramAccountChangeCallback,
  ): number {
    const id = ++this._programAccountChangeSubscriptionCounter;
    this._programAccountChangeSubscriptions[id] = {
      programId: programId.toBase58(),
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
  async removeProgramAccountChangeListener(id: number): Promise<void> {
    if (this._programAccountChangeSubscriptions[id]) {
      const subInfo = this._programAccountChangeSubscriptions[id];
      delete this._programAccountChangeSubscriptions[id];
      await this._unsubscribe(subInfo, 'programUnsubscribe');
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown program account change id: ${id}`);
    }
  }

  /**
   * @private
   */
  _wsOnSlotNotification(notification: Object) {
    const res = SlotNotificationResult(notification);
    if (res.error) {
      throw new Error('slot notification failed: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    const {parent, slot, root} = res.result;
    const keys = Object.keys(this._slotSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._slotSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        sub.callback({
          parent,
          slot,
          root,
        });
        return true;
      }
    }
  }

  /**
   * Register a callback to be invoked upon slot changes
   *
   * @param callback Function to invoke whenever the slot changes
   * @return subscription id
   */
  onSlotChange(callback: SlotChangeCallback): number {
    const id = ++this._slotSubscriptionCounter;
    this._slotSubscriptions[id] = {
      callback,
      subscriptionId: null,
    };
    this._updateSubscriptions();
    return id;
  }

  /**
   * Deregister a slot notification callback
   *
   * @param id subscription id to deregister
   */
  async removeSlotChangeListener(id: number): Promise<void> {
    if (this._slotSubscriptions[id]) {
      const subInfo = this._slotSubscriptions[id];
      delete this._slotSubscriptions[id];
      await this._unsubscribe(subInfo, 'slotUnsubscribe');
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown slot change id: ${id}`);
    }
  }

  _argsWithCommitment(args: Array<any>, override: ?Commitment): Array<any> {
    const commitment = override || this._commitment;
    if (commitment) {
      args.push({commitment});
    }
    return args;
  }

  /**
   * @private
   */
  _wsOnSignatureNotification(notification: Object) {
    const res = SignatureNotificationResult(notification);
    if (res.error) {
      throw new Error('signature notification failed: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    const keys = Object.keys(this._signatureSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._signatureSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        // Signatures subscriptions are auto-removed by the RPC service so
        // no need to explicitly send an unsubscribe message
        delete this._signatureSubscriptions[id];
        this._updateSubscriptions();
        sub.callback(res.result.value, res.result.context);
        return;
      }
    }
  }

  /**
   * Register a callback to be invoked upon signature updates
   *
   * @param signature Transaction signature string in base 58
   * @param callback Function to invoke on signature notifications
   * @return subscription id
   */
  onSignature(
    signature: TransactionSignature,
    callback: SignatureResultCallback,
  ): number {
    const id = ++this._signatureSubscriptionCounter;
    this._signatureSubscriptions[id] = {
      signature,
      callback,
      subscriptionId: null,
    };
    this._updateSubscriptions();
    return id;
  }

  /**
   * Deregister a signature notification callback
   *
   * @param id subscription id to deregister
   */
  async removeSignatureListener(id: number): Promise<void> {
    if (this._signatureSubscriptions[id]) {
      const subInfo = this._signatureSubscriptions[id];
      delete this._signatureSubscriptions[id];
      await this._unsubscribe(subInfo, 'signatureUnsubscribe');
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown signature result id: ${id}`);
    }
  }

  /**
   * @private
   */
  _wsOnRootNotification(notification: Object) {
    const res = RootNotificationResult(notification);
    if (res.error) {
      throw new Error('root notification failed: ' + res.error.message);
    }
    assert(typeof res.result !== 'undefined');
    const root = res.result;
    const keys = Object.keys(this._rootSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._rootSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        sub.callback(root);
        return true;
      }
    }
  }

  /**
   * Register a callback to be invoked upon root changes
   *
   * @param callback Function to invoke whenever the root changes
   * @return subscription id
   */
  onRootChange(callback: RootChangeCallback): number {
    const id = ++this._rootSubscriptionCounter;
    this._rootSubscriptions[id] = {
      callback,
      subscriptionId: null,
    };
    this._updateSubscriptions();
    return id;
  }

  /**
   * Deregister a root notification callback
   *
   * @param id subscription id to deregister
   */
  async removeRootChangeListener(id: number): Promise<void> {
    if (this._rootSubscriptions[id]) {
      const subInfo = this._rootSubscriptions[id];
      delete this._rootSubscriptions[id];
      await this._unsubscribe(subInfo, 'rootUnsubscribe');
      this._updateSubscriptions();
    } else {
      throw new Error(`Unknown root change id: ${id}`);
    }
  }
}
