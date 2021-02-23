// import assert from 'assert';
import bs58 from 'bs58';
// import {Buffer} from 'buffer';
import {parse as urlParse, format as urlFormat} from 'url';
import fetch from 'node-fetch';
import ClientBrowser from 'jayson/lib/client/browser';
import {
  assert as assertType,
  type,
  number,
  string,
  array,
  Struct,
  boolean,
  literal,
  union,
  any,
  optional,
  nullable,
  Describe,
  coerce,
  instance,
  create,
  tuple,
  unknown,
} from 'superstruct';
// import {Client as RpcWebSocketClient} from 'rpc-websockets';

import assert from "assert";
import {AgentManager} from './agent-manager';
// import {NonceAccount} from './nonce-account';
import {PublicKey} from './publickey';
// import {MS_PER_SLOT} from './timing';
import {Transaction} from './transaction';
import {Message} from './message';
import {sleep} from './util/sleep';
// import {promiseTimeout} from './util/promise-timeout';
// import {toBuffer} from './util/to-buffer';
import type {Blockhash} from './blockhash';
import type {FeeCalculator} from './fee-calculator';
// import type {Account} from './account';
// import type {TransactionSignature} from './transaction';
import type {CompiledInstruction} from './message';

const PublicKeyFromString = coerce(instance(PublicKey), string(),
  (value) => new PublicKey(value)
);

const BufferFromBase64 = coerce(instance(Buffer), tuple([string(), literal("base64")]),
  (value) => Buffer.from(value[0], 'base64')
);

/**
 * Transaction signature as Base58 string.
 * @public
 */
export type TransactionSignature = string;

/**
 * The expiration time of a recently cached blockhash in milliseconds.
 * @internal
 */
export const BLOCKHASH_CACHE_TIMEOUT_MS = 30 * 1000;

type RpcRequest = (methodName: string, args: Array<any>) => any;

export type TokenAccountsFilter = {
  mint: PublicKey;
} | {
  programId: PublicKey;
};

// /**
//  * Extra contextual information for RPC responses
//  *
//  * @typedef {Object} Context
//  * @property {number} slot
//  */
// type Context = {
//   slot: number;
// };

// /**
//  * Options for sending transactions
//  *
//  * @typedef {Object} SendOptions
//  * @property {boolean | undefined} skipPreflight disable transaction verification step
//  * @property {Commitment | undefined} preflightCommitment preflight commitment level
//  */
// export type SendOptions = {
//   skipPreflight?: boolean;
//   preflightCommitment?: Commitment;
// };

// /**
//  * Options for confirming transactions
//  *
//  * @typedef {Object} ConfirmOptions
//  * @property {boolean | undefined} skipPreflight disable transaction verification step
//  * @property {Commitment | undefined} commitment desired commitment level
//  * @property {Commitment | undefined} preflightCommitment preflight commitment level
//  */
// export type ConfirmOptions = {
//   skipPreflight?: boolean;
//   commitment?: Commitment;
//   preflightCommitment?: Commitment;
// };

// /**
//  * Options for getConfirmedSignaturesForAddress2
//  *
//  * @typedef {Object} ConfirmedSignaturesForAddress2Options
//  * @property {TransactionSignature | undefined} before start searching backwards from this transaction signature.
//  *               If not provided the search starts from the highest max confirmed block.
//  * @property {number | undefined} limit maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
//  *
//  */
// export type ConfirmedSignaturesForAddress2Options = {
//   before?: TransactionSignature;
//   limit?: number;
// };

/**
 * @private
 */
function jsonRpcResultAndContext<T, U>(value: Struct<T, U>) {
  return jsonRpcResult(
    type({
      context: type({
        slot: number(),
      }),
      value,
    }),
  );
}

/**
 * RPC response which includes the slot at which the operation was evaluated.
 * @public
 */
export type RpcResponseAndContext<T> = {
  context: {
    slot: number;
  };
  value: T;
}

/**
 * @private
 */
function jsonRpcResult<T, U>(result: Struct<T, U>) {
  const jsonrpc = literal('2.0');
  return union([
    type({
      jsonrpc,
      id: string(),
      error: type({
        code: any(),
        message: string(),
        data: optional(any()),
      }),
    }),
    type({
      jsonrpc,
      id: string(),
      result,
    }),
  ]);
}

/**
 * @private
 */
function notificationResultAndContext(value: Struct) {
  return type({
    context: type({
      slot: number(),
    }),
    value,
  });
}

/**
 * The level of commitment desired when querying state.
 * ```
 * 'processed': Query the most recent block which has reached 1 confirmation by the connected node
 * 'confirmed': Query the most recent block which has reached 1 confirmation by the cluster
 * 'finalized': Query the most recent block which has been finalized by the cluster
 * ```
 * @public
 */
export type Commitment =
  | 'processed'
  | 'confirmed'
  | 'finalized'
  | 'recent' // Deprecated as of v1.5.5
  | 'single' // Deprecated as of v1.5.5
  | 'singleGossip' // Deprecated as of v1.5.5
  | 'root' // Deprecated as of v1.5.5
  | 'max'; // Deprecated as of v1.5.5

/**
 * Filter for largest accounts query
 * ```
 * 'circulating':    Return the largest accounts that are part of the circulating supply
 * 'nonCirculating': Return the largest accounts that are not part of the circulating supply
 * ```
 * @public
 */
export type LargestAccountsFilter = 'circulating' | 'nonCirculating';

/**
 * Configuration object for changing `getLargestAccounts` query behavior
 * @public
 */
export interface GetLargestAccountsConfig {
  /**
   * The level of commitment desired
   */
  commitment?: Commitment;
  /**
   * Filter largest accounts by whether they are part of the circulating supply
   */
  filter?: LargestAccountsFilter;
};

/**
 * Configuration object for changing signature status query behavior.
 * @public
 */
export interface SignatureStatusConfig {
  /**
   * Enable searching status history, not needed for recent transactions.
   */
  searchTransactionHistory: boolean;
}

/**
 * Information describing a cluster node
 * @public
 */
export interface ContactInfo {
  /**
   * Identity public key of the node
   */
  pubkey: string;
  /**
   * Gossip network address for the node (null if not available)
   */
  gossip: string | null;
  /**
   * TPU network address for the node (null if not available)
   */
  tpu: string | null;
  /**
   * JSON RPC network address for the node (null if not available)
   */
  rpc: string | null;
  /**
   * Software version of the node (null if not available)
   */
  version: string | null;
};

const ContactInfo = type({
  pubkey: string(),
  gossip: nullable(string()),
  tpu: nullable(string()),
  rpc: nullable(string()),
  version: nullable(string()),
});

/**
 * Information describing a vote account.
 * @public
 */
export interface VoteAccountInfo {
  /**
   * Public key of the vote account
   */
  votePubkey: string;
  /**
   * Identity public key of the node voting with this account
   */
  nodePubkey: string;
  /**
   * The stake, in lamports, delegated to this vote account and activated
   */
  activatedStake: number;
  /**
   * Whether the vote account is staked for this epoch
   */
  epochVoteAccount: boolean;
  /**
   * Recent epoch voting credit history for this voter
   */
  epochCredits: Array<[number, number, number]>;
  /**
   * A percentage (0-100) of rewards payout owed to the voter
   */
  commission: number;
  /**
   * Most recent slot voted on by this vote account
   */
  lastVote: number;
};

const VoteAccountInfo = type({
  votePubkey: string(),
  nodePubkey: string(),
  activatedStake: number(),
  epochVoteAccount: boolean(),
  epochCredits: array(
    tuple([number(), number(), number()]),
  ),
  commission: number(),
  lastVote: number(),
  rootSlot: nullable(number()),
});

/**
 * A collection of cluster vote accounts
 * @public
 */
export interface VoteAccountStatus {
  /**
   * Active vote accounts
   */
  current: Array<VoteAccountInfo>;
  /**
   * Inactive vote accounts
   */
  delinquent: Array<VoteAccountInfo>;
};

const VoteAccountStatus = type({
  current: array(VoteAccountInfo),
  delinquent: array(VoteAccountInfo),
});

/**
 * Network Inflation
 * (see {@link https://docs.solana.com/inflation/inflation_schedule | proposed schedule})
 * @public
 */
export interface InflationGovernor {
  /**
   * The percentage of total inflation allocated to the foundation.
   */
  foundation: number;
  /**
   * The duration of foundation pool inflation in years.
   */
  foundationTerm: number;
  /**
   * The initial inflation percentage from time 0.
   */
  initial: number;
  /**
   * The rate per year at which inflation is lowered.
   */
  taper: number;
  /**
   * The terminal inflation percentage.
   */
  terminal: number;
}

const InflationGovernor: Describe<InflationGovernor> = type({
  foundation: number(),
  foundationTerm: number(),
  initial: number(),
  taper: number(),
  terminal: number(),
});

/**
 * Information about the current epoch.
 * @public
 */
export interface EpochInfo {
  /**
   * The current epoch.
   */
  epoch: number;
  /**
   * The current slot relative to the start of the current epoch.
   */
  slotIndex: number;
  /**
   * The number of slots in this epoch.
   */
  slotsInEpoch: number;
  /**
   * The current slot.
   */
  absoluteSlot: number;
  /**
   * The current block height.
   */
  blockHeight?: number;
  /**
   * The current number of transactions in the current epoch.
   */
  transactionCount?: number;
}

const EpochInfo: Describe<EpochInfo> = type({
  epoch: number(),
  slotIndex: number(),
  slotsInEpoch: number(),
  absoluteSlot: number(),
  blockHeight: optional(number()),
  transactionCount: optional(number()),
});

/**
 * Epoch schedule
 * (see {@link https://docs.solana.com/terminology#epoch | terminology})
 * @public
 */
export interface EpochSchedule {
  /**
   * The maximum number of slots in each epoch.
   */
  slotsPerEpoch: number;
  /**
   * The number of slots before beginning of an epoch to calculate a leader schedule for that epoch.
   */
  leaderScheduleSlotOffset: number;
  /**
   * Indicates whether epochs start short and grow.
   */
  warmup: boolean;
  /**
   * The first epoch with `slotsPerEpoch` slots.
   */
  firstNormalEpoch: number;
  /**
   * The first slot of `firstNormalEpoch`.
   */
  firstNormalSlot: number;
}

const EpochSchedule: Describe<EpochSchedule> = type({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

// /**
//  * Leader schedule
//  * (see https://docs.solana.com/terminology#leader-schedule)
//  *
//  * @typedef {Object} LeaderSchedule
//  */
// type LeaderSchedule = {
//   [address: string]: number[];
// };

// const GetLeaderScheduleResult = struct.record([
//   string(),
//   'any', // validating array(number()]) is extremely slow
// ]);

/**
 * Transaction error object.
 * @public
 */
export type TransactionError = object;

const TransactionError = nullable(type({}));

// /**
//  * Signature status for a transaction
//  */
// const SignatureStatusResult = type({err: TransactionErrorResult});

/**
 * Version info for a node.
 * @public
 */
export interface Version {
  /**
   * The current version of solana-core.
   */
  'solana-core': string;
  /**
   * The first 4 bytes of the FeatureSet identifier
   */
  'feature-set': number | null;
}

const Version: Describe<Version> = type({
  'solana-core': string(),
  'feature-set': nullable(number()),
});

// type SimulatedTransactionResponse = {
//   err: TransactionError | string | null;
//   logs: Array<string> | null;
// };

// const SimulatedTransactionResponseValidator = jsonRpcResultAndContext(
//   struct.pick({
//     err: struct.union(['null', 'object', string()]),
//     logs: struct.union(['null', array(string()])]),
//   }),
// );

// type ParsedInnerInstruction = {
//   index: number;
//   instructions: (ParsedInstruction | PartiallyDecodedInstruction)[];
// };

/**
 * Token amount object which returns a token amount in different formats
 * for various client use cases.
 * @public
 */
export interface TokenAmount {
  /**
   * Raw amount of tokens as string ignoring decimals.
   */
  amount: string;
  /**
   * Number of decimals configured for token's mint.
   */
  decimals: number;
  /**
   * Token account as float, accounts for decimals.
   */
  uiAmount: number;
};

const TokenAmount = type({
  amount: string(),
  uiAmount: number(),
  decimals: number(),
});

/**
 * @public
 */
export interface TokenBalance {
  accountIndex: number;
  mint: string;
  uiTokenAmount: TokenAmount;
};

const TokenBalance = type({
  accountIndex: number(),
  mint: string(),
  uiTokenAmount: TokenAmount,
});

// /**
//  * Metadata for a parsed confirmed transaction on the ledger
//  *
//  * @typedef {Object} ParsedConfirmedTransactionMeta
//  * @property {number} fee The fee charged for processing the transaction
//  * @property {Array<ParsedInnerInstruction>} innerInstructions An array of cross program invoked parsed instructions
//  * @property {Array<number>} preBalances The balances of the transaction accounts before processing
//  * @property {Array<number>} postBalances The balances of the transaction accounts after processing
//  * @property {Array<string>} logMessages An array of program log messages emitted during a transaction
//  * @property {Array<TokenBalance>} preTokenBalances The token balances of the transaction accounts before processing
//  * @property {Array<TokenBalance>} postTokenBalances The token balances of the transaction accounts after processing
//  * @property {object|null} err The error result of transaction processing
//  */
// type ParsedConfirmedTransactionMeta = {
//   fee: number;
//   innerInstructions?: ParsedInnerInstruction[];
//   preBalances: Array<number>;
//   postBalances: Array<number>;
//   logMessages?: Array<string>;
//   preTokenBalances?: Array<TokenBalance>;
//   postTokenBalances?: Array<TokenBalance>;
//   err: TransactionError | null;
// };

/**
 * Compact form of inner instruction metadata
 * @public
 */
export interface CompiledInnerInstruction {
  index: number;
  instructions: CompiledInstruction[];
};

/**
 * Metadata for a confirmed transaction on the ledger
 * @public
 */
export interface ConfirmedTransactionMeta {
  /**
   * The fee charged for processing the transaction
   */
  fee: number;
  /**
   * An array of cross program invoked instructions
   */
  innerInstructions?: CompiledInnerInstruction[] | null;
  /**
   * The balances of the transaction accounts before processing
   */
  preBalances: Array<number>;
  /**
   * The balances of the transaction accounts after processing
   */
  postBalances: Array<number>;
  /**
   * An array of program log messages emitted during a transaction
   */
  logMessages?: Array<string> | null;
  /**
   * The token balances of the transaction accounts before processing
   */
  preTokenBalances?: Array<TokenBalance> | null;
  /**
   * The token balances of the transaction accounts after processing
   */
  postTokenBalances?: Array<TokenBalance> | null;
  /**
   * The error result of transaction processing
   */
  err: TransactionError | null;
};

// /**
//  * A confirmed transaction on the ledger
//  *
//  * @typedef {Object} ConfirmedTransaction
//  * @property {number} slot The slot during which the transaction was processed
//  * @property {Transaction} transaction The details of the transaction
//  * @property {ConfirmedTransactionMeta|null} meta Metadata produced from the transaction
//  * @property {number|null|undefined} blockTime The unix timestamp of when the transaction was processed
//  */
// type ConfirmedTransaction = {
//   slot: number;
//   transaction: Transaction;
//   meta: ConfirmedTransactionMeta | null;
//   blockTime?: number | null;
// };

// /**
//  * A partially decoded transaction instruction
//  *
//  * @typedef {Object} ParsedMessageAccount
//  * @property {PublicKey} pubkey Public key of the account
//  * @property {PublicKey} accounts Indicates if the account signed the transaction
//  * @property {string} data Raw base-58 instruction data
//  */
// type PartiallyDecodedInstruction = {
//   programId: PublicKey;
//   accounts: Array<PublicKey>;
//   data: string;
// };


/**
 * A parsed transaction message account
 *
 * @typedef {Object} ParsedMessageAccount
 * @property {PublicKey} pubkey Public key of the account
 * @property {boolean} signer Indicates if the account signed the transaction
 * @property {boolean} writable Indicates if the account is writable for this transaction
 */
export interface KeyedAccountInfo<T> {
  pubkey: PublicKey;
  account: AccountInfo<T>;
};

const AccountInfo = type({
  executable: boolean(),
  owner: PublicKeyFromString,
  lamports: number(),
  data: BufferFromBase64,
  rentEpoch: number(),
});

const KeyedAccountInfo = type({
  pubkey: PublicKeyFromString,
  account: AccountInfo,
});

const ParsedAccountInfo = type({
  executable: boolean(),
  owner: PublicKeyFromString,
  lamports: number(),
  data: union([
    BufferFromBase64,
    type({
      program: string(),
      parsed: unknown(),
      space: number(),
    }),
  ]),
  rentEpoch: number(),
});

const KeyedParsedAccountInfo = type({
  pubkey: PublicKeyFromString,
  account: ParsedAccountInfo,
});

// /**
//  * A parsed transaction message account
//  *
//  * @typedef {Object} ParsedMessageAccount
//  * @property {PublicKey} pubkey Public key of the account
//  * @property {boolean} signer Indicates if the account signed the transaction
//  * @property {boolean} writable Indicates if the account is writable for this transaction
//  */
// type ParsedMessageAccount = {
//   pubkey: PublicKey;
//   signer: boolean;
//   writable: boolean;
// };

// /**
//  * A parsed transaction instruction
//  *
//  * @typedef {Object} ParsedInstruction
//  * @property {string} program Name of the program for this instruction
//  * @property {PublicKey} programId ID of the program for this instruction
//  * @property {any} parsed Parsed instruction info
//  */
// type ParsedInstruction = {
//   program: string;
//   programId: PublicKey;
//   parsed: any;
// };

// /**
//  * A parsed transaction message
//  *
//  * @typedef {Object} ParsedMessage
//  * @property {Array<ParsedMessageAccount>} accountKeys Accounts used in the instructions
//  * @property {Array<ParsedInstruction | PartiallyDecodedInstruction>} instructions The atomically executed instructions for the transaction
//  * @property {string} recentBlockhash Recent blockhash
//  */
// type ParsedMessage = {
//   accountKeys: ParsedMessageAccount[];
//   instructions: (ParsedInstruction | PartiallyDecodedInstruction)[];
//   recentBlockhash: string;
// };

// /**
//  * A parsed transaction
//  *
//  * @typedef {Object} ParsedTransaction
//  * @property {Array<string>} signatures Signatures for the transaction
//  * @property {ParsedMessage} message Message of the transaction
//  */
// type ParsedTransaction = {
//   signatures: Array<string>;
//   message: ParsedMessage;
// };

// /**
//  * A parsed and confirmed transaction on the ledger
//  *
//  * @typedef {Object} ParsedConfirmedTransaction
//  * @property {number} slot The slot during which the transaction was processed
//  * @property {ParsedTransaction} transaction The details of the transaction
//  * @property {ConfirmedTransactionMeta|null} meta Metadata produced from the transaction
//  * @property {number|null|undefined} blockTime The unix timestamp of when the transaction was processed
//  */
// type ParsedConfirmedTransaction = {
//   slot: number;
//   transaction: ParsedTransaction;
//   meta: ParsedConfirmedTransactionMeta | null;
//   blockTime?: number | null;
// };

const ConfirmedTransactionMetaResult = type({
  err: TransactionError,
  fee: number(),
  innerInstructions: optional(nullable(
    array(
      type({
        index: number(),
        instructions: array(
          type({
            accounts: array(number()),
            data: string(),
            programIdIndex: number(),
          }),
        ),
      }),
    ),
  )),
  preBalances: array(number()),
  postBalances: array(number()),
  logMessages: optional(nullable(array(string()))),
  preTokenBalances: optional(nullable(
    array(TokenBalance)
  )),
  postTokenBalances: optional(nullable(
    array(TokenBalance)
  )),
});

const ConfirmedTransactionResult = type({
  signatures: array(string()),
  message: type({
    accountKeys: array(string()),
    header: type({
      numRequiredSignatures: number(),
      numReadonlySignedAccounts: number(),
      numReadonlyUnsignedAccounts: number(),
    }),
    instructions: array(
      type({
        accounts: array(number()),
        data: string(),
        programIdIndex: number(),
      }),
    ),
    recentBlockhash: string(),
  }),
});

const TransactionFromConfirmed = coerce(instance(Transaction), ConfirmedTransactionResult,
  (result) => {
    const {message, signatures} = result;
    return Transaction.populate(new Message(message), signatures);
  }
);

/**
 * A ConfirmedBlock on the ledger
 */
export interface ConfirmedBlock {
  /**
   * Blockhash of this block
   */
  blockhash: Blockhash;
  /**
   * Blockhash of this block's parent
   */
  previousBlockhash: Blockhash;
  /**
   * Slot index of this block's parent
   */
  parentSlot: number;
  /**
   * Vector of transactions and status metas
   */
  transactions: Array<{
    transaction: Transaction;
    meta: ConfirmedTransactionMeta | null;
  }>;
  /**
   * Vector of block rewards
   */
  rewards: Array<{
    pubkey: string;
    lamports: number;
    postBalance: number | null;
    rewardType: string | null;
  }>;
};

const ConfirmedBlock = type({
  blockhash: string(),
  previousBlockhash: string(),
  parentSlot: number(),
  transactions: array(
    type({
      transaction: TransactionFromConfirmed,
      meta: nullable(ConfirmedTransactionMetaResult),
    }),
  ),
  rewards: optional(
    array(
      type({
        pubkey: string(),
        lamports: number(),
        postBalance: nullable(number()),
        rewardType: nullable(string()),
      }),
    ),
  ),
});

/**
 * A performance sample
 * @public
 */
export interface PerfSample {
  /**
   * The slot number of the sample.
   */
  slot: number;
  /**
   * The number of transactions in the sample period.
   */
  numTransactions: number;
  /**
   * The number of slots in the sample period.
   */
  numSlots: number;
  /**
   * The sample period in seconds.
   */
  samplePeriodSecs: number;
}

const PerfSample: Describe<PerfSample> = type({
  slot: number(),
  numTransactions: number(),
  numSlots: number(),
  samplePeriodSecs: number(),
});

function createRpcRequest(url: string, useHttps: boolean): RpcRequest {
  let agentManager: AgentManager;
  if (!process.env.BROWSER) {
    agentManager = new AgentManager(useHttps);
  }

  const server = new ClientBrowser(async (request, callback) => {
    const agent = agentManager ? agentManager.requestStart() : undefined;
    const options = {
      method: 'POST',
      body: request,
      agent,
      headers: {
        'Content-Type': 'application/json',
      },
    };

    try {
      let too_many_requests_retries = 5;
      let res;
      let waitTime = 500;
      for (;;) {
        res = await fetch(url, options);
        if (res.status !== 429 /* Too many requests */) {
          break;
        }
        too_many_requests_retries -= 1;
        if (too_many_requests_retries === 0) {
          break;
        }
        console.log(
          `Server responded with ${res.status} ${res.statusText}.  Retrying after ${waitTime}ms delay...`,
        );
        await sleep(waitTime);
        waitTime *= 2;
      }

      const text = await res.text();
      if (res.ok) {
        callback(null, text);
      } else {
        callback(new Error(`${res.status} ${res.statusText}: ${text}`));
      }
    } catch (err) {
      callback(err);
    } finally {
      agentManager && agentManager.requestEnd();
    }
  }, {});

  return (method, args) => {
    return new Promise((resolve, reject) => {
      server.request(method, args, (err: any, response: any) => {
        if (err) {
          reject(err);
          return;
        }
        resolve(response);
      });
    });
  };
}

// /**
//  * Expected JSON RPC response for the "getLeaderSchedule" message
//  */
// const GetLeaderScheduleRpcResult = jsonRpcResult(GetLeaderScheduleResult);

/**
 * Supply
 */
export interface Supply {
  /**
   * Total supply in lamports.
   */
  total: number;
  /**
   * Circulating supply in lamports.
   */
  circulating: number;
  /**
   * Non-circulating supply in lamports.
   */
  nonCirculating: number;
  /**
   * List of non-circulating account addresses
   */
  nonCirculatingAccounts: Array<PublicKey>;
};

/**
 * Expected JSON RPC response for the "getSupply" message
 */
const Supply: Describe<Supply> = type({
  total: number(),
  circulating: number(),
  nonCirculating: number(),
  nonCirculatingAccounts: array(PublicKeyFromString),
});

/**
 * Token address and balance.
 * @public
 */
export interface TokenAccountBalancePair {
  /**
   * Address of the token account
   */
  address: PublicKey;
  /**
   * Raw amount of tokens as string ignoring decimals
   */
  amount: string;
  /**
   * Number of decimals configured for token's mint
   */
  decimals: number;
  /**
   * Token account as float, accounts for decimals
   */
  uiAmount: number;
};

/**
 * Expected JSON RPC response for the "getTokenLargestAccounts" message
 */
const LargestTokenAcccounts = array(
  type({
    address: PublicKeyFromString,
    amount: string(),
    uiAmount: number(),
    decimals: number(),
  }),
);

// /**
//  * Expected JSON RPC response for the "getTokenAccountBalance" message
//  */
// const GetTokenAccountBalance = jsonRpcResultAndContext(TokenAmountResult);

// /**
//  * Expected JSON RPC response for the "getTokenSupply" message
//  */
// const GetTokenSupplyRpcResult = jsonRpcResultAndContext(TokenAmountResult);

/**
 * Expected JSON RPC response for the "getTokenAccountsByOwner" message
 */
const GetTokenAccountsByOwner = array(
  type({
    pubkey: PublicKeyFromString,
    account: type({
      executable: boolean(),
      owner: PublicKeyFromString,
      lamports: number(),
      data: BufferFromBase64,
      rentEpoch: number(),
    }),
  }),
);

/**
 * Expected JSON RPC response for the "getTokenAccountsByOwner" message with parsed data
 */
const GetParsedTokenAccountsByOwner = array(
  type({
    pubkey: PublicKeyFromString,
    account: type({
      executable: boolean(),
      owner: PublicKeyFromString,
      lamports: number(),
      data: type({
        program: string(),
        parsed: unknown(),
        space: number(),
      }),
      rentEpoch: number(),
    }),
  }),
);

/**
 * Pair of an account address and its balance
 * @public
 */
export interface AccountBalancePair {
  /**
   * The account address
   */
  address: PublicKey;
  /**
   * The account balance
   */
  lamports: number;
};

/**
 * Expected JSON RPC response for the "getLargestAccounts" message
 */
const LargestAccounts = array(
  type({
    lamports: number(),
    address: PublicKeyFromString,
  }),
);

/**
 * Stake Activation data
 * @public
 *
 * @typedef {Object} StakeActivationData
 * @property {string} state: <string - 
 * @property {number} active: 
 * @property {number} inactive: 
 */
type StakeActivationData = {
  /**
   * The stake account's activation state.
   */
  state: 'active' | 'inactive' | 'activating' | 'deactivating';
  /**
   * The amount of active stake during the epoch.
   */
  active: number;
  /**
   * The amount of inactive stake during the epoch.
   */
  inactive: number;
};

const StakeActivationData = type({
  state: union([
    literal('active'),
    literal('inactive'),
    literal('activating'),
    literal('deactivating'),
  ]),
  active: number(),
  inactive: number(),
});

// /**
//  * Expected JSON RPC response for the "getConfirmedSignaturesForAddress" message
//  */
// const GetConfirmedSignaturesForAddressRpcResult = jsonRpcResult(
//   array(string()]),
// );

// /**
//  * Expected JSON RPC response for the "getConfirmedSignaturesForAddress2" message
//  */

// const GetConfirmedSignaturesForAddress2RpcResult = jsonRpcResult(
//   array(
//     struct.pick({
//       signature: string(),
//       slot: number(),
//       err: TransactionErrorResult,
//       memo: struct.union(['null', string()]),
//       blockTime: struct.union(['undefined', 'null', number()]),
//     }),
//   ]),
// );

// /***
//  * Expected JSON RPC response for the "accountNotification" message
//  */
// const AccountNotificationResult = type({
//   subscription: number(),
//   result: notificationResultAndContext(AccountInfoResult),
// });

// /***
//  * Expected JSON RPC response for the "programNotification" message
//  */
// const ProgramAccountNotificationResult = type({
//   subscription: number(),
//   result: notificationResultAndContext(ProgramAccountInfoResult),
// });

// /**
//  * @private
//  */
// const SlotInfoResult = type({
//   parent: number(),
//   slot: number(),
//   root: number(),
// });

// /**
//  * Expected JSON RPC response for the "slotNotification" message
//  */
// const SlotNotificationResult = type({
//   subscription: number(),
//   result: SlotInfoResult,
// });

// /**
//  * Expected JSON RPC response for the "signatureNotification" message
//  */
// const SignatureNotificationResult = type({
//   subscription: number(),
//   result: notificationResultAndContext(SignatureStatusResult),
// });

// /**
//  * Expected JSON RPC response for the "rootNotification" message
//  */
// const RootNotificationResult = type({
//   subscription: number(),
//   result: number(),
// });

// /**
//  * Expected JSON RPC response for the "getProgramAccounts" message
//  */
// const GetParsedProgramAccountsRpcResult = jsonRpcResult(
//   array(ParsedProgramAccountInfoResult]),
// );

// /**
//  * Expected JSON RPC response for the "getSlot" message
//  */
// const GetSlot = jsonRpcResult(number());

// /**
//  * Expected JSON RPC response for the "getSlotLeader" message
//  */
// const GetSlotLeader = jsonRpcResult(string());


// /**
//  * Expected JSON RPC response for the "getSignatureStatuses" message
//  */
// const GetSignatureStatusesRpcResult = jsonRpcResultAndContext(
//   array(
//     struct.union([
//       'null',
//       struct.pick({
//         slot: number(),
//         confirmations: struct.union([number(), 'null']),
//         err: TransactionErrorResult,
//         confirmationStatus: 'string?',
//       }),
//     ]),
//   ]),
// );

// /**
//  * Expected JSON RPC response for the "getTransactionCount" message
//  */
// const GetTransactionCountRpcResult = jsonRpcResult(number());

// /**
//  * Expected JSON RPC response for the "getTotalSupply" message
//  */
// const GetTotalSupplyRpcResult = jsonRpcResult(number());


// /**
//  * @private
//  */
// const ParsedConfirmedTransactionResult = type({
//   signatures: array(string()]),
//   message: type({
//     accountKeys: array(
//       type({
//         pubkey: string(),
//         signer: boolean(),
//         writable: boolean(),
//       }),
//     ]),
//     instructions: array(
//       struct.union([
//         type({
//           accounts: array(string()]),
//           data: string(),
//           programId: string(),
//         }),
//         type({
//           parsed: 'any',
//           program: string(),
//           programId: string(),
//         }),
//       ]),
//     ]),
//     recentBlockhash: string(),
//   }),
// });

// /**
//  * @private
//  */
// const ParsedConfirmedTransactionMetaResult = struct.union([
//   'null',
//   struct.pick({
//     err: TransactionErrorResult,
//     fee: number(),
//     innerInstructions: struct.union([
//       array(
//         type({
//           index: number(),
//           instructions: array(
//             struct.union([
//               type({
//                 accounts: array(string()]),
//                 data: string(),
//                 programId: string(),
//               }),
//               type({
//                 parsed: 'any',
//                 program: string(),
//                 programId: string(),
//               }),
//             ]),
//           ]),
//         }),
//       ]),
//       'null',
//       'undefined',
//     ]),
//     preBalances: array(number()]),
//     postBalances: array(number()]),
//     logMessages: struct.union([array(string()]), 'null', 'undefined']),
//     preTokenBalances: struct.union([
//       array(
//         struct.pick({
//           accountIndex: number(),
//           mint: string(),
//           uiTokenAmount: struct.pick({
//             amount: string(),
//             decimals: number(),
//             uiAmount: number(),
//           }),
//         }),
//       ]),
//       'null',
//       'undefined',
//     ]),
//     postTokenBalances: struct.union([
//       array(
//         struct.pick({
//           accountIndex: number(),
//           mint: string(),
//           uiTokenAmount: struct.pick({
//             amount: string(),
//             decimals: number(),
//             uiAmount: number(),
//           }),
//         }),
//       ]),
//       'null',
//       'undefined',
//     ]),
//   }),
// ]);


// /**
//  * Expected JSON RPC response for the "getConfirmedTransaction" message
//  */
// const GetConfirmedTransactionRpcResult = jsonRpcResult(
//   struct.union([
//     'null',
//     struct.pick({
//       slot: number(),
//       transaction: ConfirmedTransactionResult,
//       meta: nullable(ConfirmedTransactionMetaResult),
//       blockTime: struct.union([number(), 'null', 'undefined']),
//     }),
//   ]),
// );

// /**
//  * Expected JSON RPC response for the "getConfirmedTransaction" message
//  */
// const GetParsedConfirmedTransactionRpcResult = jsonRpcResult(
//   struct.union([
//     'null',
//     struct.pick({
//       slot: number(),
//       transaction: ParsedConfirmedTransactionResult,
//       meta: ParsedConfirmedTransactionMetaResult,
//       blockTime: struct.union([number(), 'null', 'undefined']),
//     }),
//   ]),
// );

/**
 * Recent string blockhash and fee calculator
 * @public
 */
export interface RecentBlockhash {
  /**
   * A blockhash as base-58 encoded string.
   */
  blockhash: Blockhash;
  /**
   * The fee calculator for this blockhash.
   */
  feeCalculator: FeeCalculator;
}

const RecentBlockhash: Describe<RecentBlockhash> = type({
  blockhash: string(),
  feeCalculator: type({
    lamportsPerSignature: number(),
  }),
});

const GetFeeCalculatorForBlockhash = nullable(
  type({
    feeCalculator: type({
      lamportsPerSignature: number(),
    }),
  }),
);

// /**
//  * Expected JSON RPC response for the "sendTransaction" message
//  */
// const SendTransactionRpcResult = jsonRpcResult(string());

// /**
//  * Information about the latest slot being processed by a node
//  *
//  * @typedef {Object} SlotInfo
//  * @property {number} slot Currently processing slot
//  * @property {number} parent Parent of the current slot
//  * @property {number} root The root block of the current slot's fork
//  */
// export type SlotInfo = {
//   slot: number;
//   parent: number;
//   root: number;
// };

/**
 * Parsed account data
 * @public
 */
export interface ParsedAccountData {
  /**
   * Name of the program that owns this account.
   */
  program: string;
  /**
   * Parsed account data object.
   */
  parsed: unknown;
  /**
   * Space used by account data.
   */
  space: number;
};

/**
 * Information describing an account
 * @public
 */
export interface AccountInfo<T> {
  /**
   * Whether this account's data contains a loaded program
   */
  executable: boolean;
  /**
   * Identifier of the program that owns the account.
   */
  owner: PublicKey;
  /**
   * Number of lamports assigned to the account.
   */
  lamports: number;
  /**
   * The data assigned to the account.
   */
  data: T;
  /**
   * The epoch at which this account will next owe rent
   */
  rentEpoch: number;
};

// /**
//  * Account information identified by pubkey
//  *
//  * @typedef {Object} KeyedAccountInfo
//  * @property {PublicKey} accountId
//  * @property {AccountInfo<Buffer>} accountInfo
//  */
// export type KeyedAccountInfo = {
//   accountId: PublicKey;
//   accountInfo: AccountInfo<Buffer>;
// };

// /**
//  * Callback function for account change notifications
//  */
// export type AccountChangeCallback = (
//   accountInfo: AccountInfo<Buffer>,
//   context: Context,
// ) => void;

// /**
//  * @private
//  */
// type SubscriptionId = 'subscribing' | number;

// /**
//  * @private
//  */
// type AccountSubscriptionInfo = {
//   publicKey: string; // PublicKey of the account as a base 58 string
//   callback: AccountChangeCallback;
//   commitment?: Commitment;
//   subscriptionId: ?SubscriptionId; // null when there's no current server subscription id
// };

// /**
//  * Callback function for program account change notifications
//  */
// export type ProgramAccountChangeCallback = (
//   keyedAccountInfo: KeyedAccountInfo,
//   context: Context,
// ) => void;

// /**
//  * @private
//  */
// type ProgramAccountSubscriptionInfo = {
//   programId: string; // PublicKey of the program as a base 58 string
//   callback: ProgramAccountChangeCallback;
//   commitment?: Commitment;
//   subscriptionId: ?SubscriptionId; // null when there's no current server subscription id
// };

// /**
//  * Callback function for slot change notifications
//  */
// export type SlotChangeCallback = (slotInfo: SlotInfo) => void;

// /**
//  * @private
//  */
// type SlotSubscriptionInfo = {
//   callback: SlotChangeCallback;
//   subscriptionId: ?SubscriptionId; // null when there's no current server subscription id
// };

// /**
//  * Callback function for signature notifications
//  */
// export type SignatureResultCallback = (
//   signatureResult: SignatureResult,
//   context: Context,
// ) => void;

// /**
//  * @private
//  */
// type SignatureSubscriptionInfo = {
//   signature: TransactionSignature; // TransactionSignature as a base 58 string
//   callback: SignatureResultCallback;
//   commitment?: Commitment;
//   subscriptionId: ?SubscriptionId; // null when there's no current server subscription id
// };

// /**
//  * Callback function for root change notifications
//  */
// export type RootChangeCallback = (root: number) => void;

// /**
//  * @private
//  */
// type RootSubscriptionInfo = {
//   callback: RootChangeCallback;
//   subscriptionId: ?SubscriptionId; // null when there's no current server subscription id
// };

/**
 * Signature result
 */
export interface SignatureResult {
  /**
   * The transaction error or null if the transaction was successful.
   */
  err: TransactionError | null;
};

/**
 * Transaction signature status.
 * @public
 */
export interface SignatureStatus {
  /**
   * The slot when the transaction was processed;
   */
  slot: number;
  /**
   * The number of blocks that have been confirmed and voted on in the fork containing `slot`. (TODO)
   */
  confirmations: number | null;
  /**
   * The transaction error if the transaction failed.
   */
  err: TransactionError | null;
  /**
   * The transaction's cluster confirmation status, if data available. Possible non-null responses: `processed`, `confirmed`, `finalized`.
   */
  confirmationStatus?: string | null;
};

const SignatureStatus: Describe<SignatureStatus> = type({
  slot: number(),
  confirmations: nullable(number()),
  err: TransactionError,
  confirmationStatus: optional(nullable(string())),
});

// /**
//  * A confirmed signature with its status
//  *
//  * @typedef {Object} ConfirmedSignatureInfo
//  * @property {string} signature the transaction signature
//  * @property {number} slot when the transaction was processed
//  * @property {TransactionError | null} err error, if any
//  * @property {string | null} memo memo associated with the transaction, if any
//  * @property {number | null | undefined} blockTime The unix timestamp of when the transaction was processed
//  */
// export type ConfirmedSignatureInfo = {
//   signature: string;
//   slot: number;
//   err: TransactionError | null;
//   memo: string | null;
//   blockTime?: number | null;
// };

/**
 * A connection to a fullnode JSON RPC endpoint.
 * @public
 */
export class Connection {
  private _rpcEndpoint: string;
  private _rpcRequest: RpcRequest;
  // _rpcWebSocket: RpcWebSocketClient;
  // _rpcWebSocketConnected: boolean = false;
  // _rpcWebSocketHeartbeat: IntervalID | null = null;
  // _rpcWebSocketIdleTimeout: TimeoutID | null = null;
  private _commitment: Commitment | undefined;
  // _blockhashInfo: {
  //   recentBlockhash: Blockhash | null;
  //   lastFetch: Date;
  //   simulatedSignatures: Array<string>;
  //   transactionSignatures: Array<string>;
  // };
  // _disableBlockhashCaching: boolean = false;
  // _pollingBlockhash: boolean = false;
  // _accountChangeSubscriptions: Map<number, AccountSubscriptionInfo> = new Map();
  // _accountChangeSubscriptionCounter: number = 0;
  // _programAccountChangeSubscriptions: Map<
  //   number,
  //   ProgramAccountSubscriptionInfo
  // > = new Map();
  // _programAccountChangeSubscriptionCounter: number = 0;
  // _slotSubscriptions: Map<number, SlotSubscriptionInfo> = new Map();
  // _slotSubscriptionCounter: number = 0;
  // _signatureSubscriptions: Map<number, SignatureSubscriptionInfo> = new Map();
  // _signatureSubscriptionCounter: number = 0;
  // _rootSubscriptions: Map<number, RootSubscriptionInfo> = new Map();
  // _rootSubscriptionCounter: number = 0;

  /**
   * Establish a JSON RPC connection
   *
   * @param endpoint - URL to the fullnode JSON RPC endpoint
   * @param commitment - optional default commitment level
   */
  constructor(endpoint: string, commitment?: Commitment) {
    this._rpcEndpoint = endpoint;

    let url = urlParse(endpoint);
    const useHttps = url.protocol === 'https:';

    this._rpcRequest = createRpcRequest(url.href, useHttps);
    this._commitment = commitment;
    // this._blockhashInfo = {
    //   recentBlockhash: null,
    //   lastFetch: new Date(0),
    //   transactionSignatures: [],
    //   simulatedSignatures: [],
    // };

    // url.protocol = useHttps ? 'wss:' : 'ws:';
    // url.host = '';
    // // Only shift the port by +1 as a convention for ws(s) only if given endpoint
    // // is explictly specifying the endpoint port (HTTP-based RPC), assuming
    // // we're directly trying to connect to solana-validator's ws listening port.
    // // When the endpoint omits the port, we're connecting to the protocol
    // // default ports: http(80) or https(443) and it's assumed we're behind a reverse
    // // proxy which manages WebSocket upgrade and backend port redirection.
    // if (url.port !== null) {
    //   url.port = String(Number(url.port) + 1);
    // }
    // this._rpcWebSocket = new RpcWebSocketClient(urlFormat(url), {
    //   autoconnect: false,
    //   max_reconnects: Infinity,
    // });
    // this._rpcWebSocket.on('open', this._wsOnOpen.bind(this));
    // this._rpcWebSocket.on('error', this._wsOnError.bind(this));
    // this._rpcWebSocket.on('close', this._wsOnClose.bind(this));
    // this._rpcWebSocket.on(
    //   'accountNotification',
    //   this._wsOnAccountNotification.bind(this),
    // );
    // this._rpcWebSocket.on(
    //   'programNotification',
    //   this._wsOnProgramAccountNotification.bind(this),
    // );
    // this._rpcWebSocket.on(
    //   'slotNotification',
    //   this._wsOnSlotNotification.bind(this),
    // );
    // this._rpcWebSocket.on(
    //   'signatureNotification',
    //   this._wsOnSignatureNotification.bind(this),
    // );
    // this._rpcWebSocket.on(
    //   'rootNotification',
    //   this._wsOnRootNotification.bind(this),
    // );
  }

  /**
   * The default commitment used for requests
   */
  get commitment(): Commitment | undefined {
    return this._commitment;
  }

  /**
   * Fetch the balance for the specified public key, return with context
   * 
   * @param publicKey - public key to query
   * @param commitment - optional default commitment level
   */
  async getBalanceAndContext(
    publicKey: PublicKey,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<number>> {
    const args = this._buildArgs([publicKey.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getBalance', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(number()));
    if ('error' in res) {
      throw new Error(
        'failed to get balance for ' +
          publicKey.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch the balance for the specified public key
   */
  async getBalance(
    publicKey: PublicKey,
    commitment?: Commitment,
  ): Promise<number> {
    try {
      const res = await this.getBalanceAndContext(publicKey, commitment);
      return res.value;
    } catch (e) {
      throw new Error(
        'failed to get balance of account ' + publicKey.toBase58() + ': ' + e,
      );
    }
  }

  /**
   * Fetch the estimated production time of a block
   */
  async getBlockTime(slot: number): Promise<number | null> {
    const unsafeRes = await this._rpcRequest('getBlockTime', [slot]);
    const res = create(unsafeRes, jsonRpcResult(nullable(number())));
    if ('error' in res) {
      throw new Error(
        'failed to get block time for slot ' + slot + ': ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch the lowest slot that the node has information about in its ledger.
   * This value may increase over time if the node is configured to purge older ledger data
   */
  async getMinimumLedgerSlot(): Promise<number> {
    const unsafeRes = await this._rpcRequest('minimumLedgerSlot', []);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if ('error' in res) {
      throw new Error(
        'failed to get minimum ledger slot: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch the slot of the lowest confirmed block that has not been purged from the ledger
   */
  async getFirstAvailableBlock(): Promise<number> {
    const unsafeRes = await this._rpcRequest('getFirstAvailableBlock', []);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if ('error' in res) {
      throw new Error(
        'failed to get first available block: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch information about the current supply
   */
  async getSupply(
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<Supply>> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSupply', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(Supply));
    if ('error' in res) {
      throw new Error('failed to get supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current supply of a token mint
   */
  async getTokenSupply(
    tokenMintAddress: PublicKey,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<TokenAmount>> {
    const args = this._buildArgs([tokenMintAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenSupply', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(TokenAmount));
    if ('error' in res) {
      throw new Error('failed to get token supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current balance of a token account
   */
  async getTokenAccountBalance(
    tokenAddress: PublicKey,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<TokenAmount>> {
    const args = this._buildArgs([tokenAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenAccountBalance', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(TokenAmount));
    if ('error' in res) {
      throw new Error(
        'failed to get token account balance: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the token accounts owned by the specified account
   */
  async getTokenAccountsByOwner(
    ownerAddress: PublicKey,
    filter: TokenAccountsFilter,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<Array<KeyedAccountInfo<Buffer>>>> {
    let _args: Array<any> = [ownerAddress.toBase58()];
    if ('mint' in filter) {
      _args.push({mint: filter.mint.toBase58()});
    } else {
      _args.push({programId: filter.programId.toBase58()});
    }

    const args = this._buildArgs(_args, commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getTokenAccountsByOwner', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(GetTokenAccountsByOwner));
    if ('error' in res) {
      throw new Error(
        'failed to get token accounts owned by account ' +
          ownerAddress.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch parsed token accounts owned by the specified account
   *
   * @return {Promise<RpcResponseAndContext<Array<{pubkey: PublicKey, account: AccountInfo<ParsedAccountData>}>>>}
   */
  async getParsedTokenAccountsByOwner(
    ownerAddress: PublicKey,
    filter: TokenAccountsFilter,
    commitment?: Commitment,
  ): Promise<
    RpcResponseAndContext<
      Array<KeyedAccountInfo<ParsedAccountData>>
    >
  > {
    let _args: Array<any> = [ownerAddress.toBase58()];
    if ('mint' in filter) {
      _args.push({mint: filter.mint.toBase58()});
    } else {
      _args.push({programId: filter.programId.toBase58()});
    }

    const args = this._buildArgs(_args, commitment, 'jsonParsed');
    const unsafeRes = await this._rpcRequest('getTokenAccountsByOwner', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(GetParsedTokenAccountsByOwner));
    if ('error' in res) {
      throw new Error(
        'failed to get token accounts owned by account ' +
          ownerAddress.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch the 20 largest accounts with their current balances
   */
  async getLargestAccounts(
    config?: GetLargestAccountsConfig,
  ): Promise<RpcResponseAndContext<Array<AccountBalancePair>>> {
    const arg = {
      ...config,
      commitment: (config && config.commitment) || this.commitment,
    };
    const args = arg.filter || arg.commitment ? [arg] : [];
    const unsafeRes = await this._rpcRequest('getLargestAccounts', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(LargestAccounts));
    if ('error' in res) {
      throw new Error('failed to get largest accounts: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the 20 largest token accounts with their current balances
   * for a given mint.
   */
  async getTokenLargestAccounts(
    mintAddress: PublicKey,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<Array<TokenAccountBalancePair>>> {
    const args = this._buildArgs([mintAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenLargestAccounts', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(LargestTokenAcccounts));
    if ('error' in res) {
      throw new Error(
        'failed to get token largest accounts: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the account info for the specified public key, return with context
   */
  async getAccountInfoAndContext(
    publicKey: PublicKey,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<AccountInfo<Buffer> | null>> {
    const args = this._buildArgs([publicKey.toBase58()], commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getAccountInfo', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(nullable(AccountInfo)));
    if ('error' in res) {
      throw new Error(
        'failed to get info about account ' +
          publicKey.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch parsed account info for the specified public key
   */
  async getParsedAccountInfo(
    publicKey: PublicKey,
    commitment?: Commitment,
  ): Promise<
    RpcResponseAndContext<AccountInfo<Buffer | ParsedAccountData> | null>
  > {
    const args = this._buildArgs(
      [publicKey.toBase58()],
      commitment,
      'jsonParsed',
    );
    const unsafeRes = await this._rpcRequest('getAccountInfo', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(nullable(ParsedAccountInfo)));
    if ('error' in res) {
      throw new Error(
        'failed to get info about account ' +
          publicKey.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the account info for the specified public key
   */
  async getAccountInfo(
    publicKey: PublicKey,
    commitment?: Commitment,
  ): Promise<AccountInfo<Buffer> | null> {
    try {
      const res = await this.getAccountInfoAndContext(publicKey, commitment);
      return res.value;
    } catch (e) {
      throw new Error(
        'failed to get info about account ' + publicKey.toBase58() + ': ' + e,
      );
    }
  }

  /**
   * Returns epoch activation information for a delegated stake account.
   * 
   * @param commitment - optional default commitment level
   * @param epoch - optional epoch to query, defaults to current epoch
   */
  async getStakeActivation(
    publicKey: PublicKey,
    commitment?: Commitment,
    epoch?: number,
  ): Promise<StakeActivationData> {
    const args = this._buildArgs(
      [publicKey.toBase58()],
      commitment,
      undefined,
      epoch !== undefined ? {epoch} : undefined,
    );

    const unsafeRes = await this._rpcRequest('getStakeActivation', args);
    const res = create(unsafeRes, jsonRpcResult(StakeActivationData));
    if ('error' in res) {
      throw new Error(
        `failed to get stake activation data for ${publicKey.toBase58()}: ${
          res.error.message
        }`,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the accounts owned by the specified program id
   */
  async getProgramAccounts(
    programId: PublicKey,
    commitment?: Commitment,
  ): Promise<Array<KeyedAccountInfo<Buffer>>> {
    const args = this._buildArgs([programId.toBase58()], commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getProgramAccounts', args);
    const res = create(unsafeRes, jsonRpcResult(array(KeyedAccountInfo)));
    if ('error' in res) {
      throw new Error(
        'failed to get accounts owned by program ' +
          programId.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch and parse all the accounts owned by the specified program id
   */
  async getParsedProgramAccounts(
    programId: PublicKey,
    commitment?: Commitment,
  ): Promise<
    Array<KeyedAccountInfo<Buffer | ParsedAccountData>>
  > {
    const args = this._buildArgs(
      [programId.toBase58()],
      commitment,
      'jsonParsed',
    );
    const unsafeRes = await this._rpcRequest('getProgramAccounts', args);
    const res = create(unsafeRes, jsonRpcResult(array(KeyedParsedAccountInfo)));
    if ('error' in res) {
      throw new Error(
        'failed to get accounts owned by program ' +
          programId.toBase58() +
          ': ' +
          res.error.message,
      );
    }
    return res.result;
  }

  // /**
  //  * Confirm the transaction identified by the specified signature.
  //  */
  // async confirmTransaction(
  //   signature: TransactionSignature,
  //   commitment?: Commitment,
  // ): Promise<RpcResponseAndContext<SignatureResult>> {
  //   let decodedSignature;
  //   try {
  //     decodedSignature = bs58.decode(signature);
  //   } catch (err) {
  //     throw new Error('signature must be base58 encoded: ' + signature);
  //   }

  //   assert(decodedSignature.length === 64, 'signature has invalid length');

  //   const start = Date.now();
  //   const subscriptionCommitment = commitment || this.commitment;

  //   let subscriptionId;
  //   let response: RpcResponseAndContext<SignatureResult> | null = null;
  //   const confirmPromise = new Promise((resolve, reject) => {
  //     try {
  //       subscriptionId = this.onSignature(
  //         signature,
  //         (result, context) => {
  //           subscriptionId = undefined;
  //           response = {
  //             context,
  //             value: result,
  //           };
  //           resolve();
  //         },
  //         subscriptionCommitment,
  //       );
  //     } catch (err) {
  //       reject(err);
  //     }
  //   });

  //   let timeoutMs = 60 * 1000;
  //   switch (subscriptionCommitment) {
  //     case 'recent':
  //     case 'single':
  //     case 'singleGossip': {
  //       timeoutMs = 30 * 1000;
  //       break;
  //     }
  //     // exhaust enums to ensure full coverage
  //     case 'max':
  //     case 'root':
  //   }

  //   try {
  //     await promiseTimeout(confirmPromise, timeoutMs);
  //   } finally {
  //     if (subscriptionId) {
  //       this.removeSignatureListener(subscriptionId);
  //     }
  //   }

  //   if (response === null) {
  //     const duration = (Date.now() - start) / 1000;
  //     throw new Error(
  //       `Transaction was not confirmed in ${duration.toFixed(
  //         2,
  //       )} seconds. It is unknown if it succeeded or failed. Check signature ${signature} using the Solana Explorer or CLI tools.`,
  //     );
  //   }

  //   return response;
  // }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getClusterNodes(): Promise<Array<ContactInfo>> {
    const unsafeRes = await this._rpcRequest('getClusterNodes', []);

    const res = create(unsafeRes, jsonRpcResult(array(ContactInfo)));
    if ('error' in res) {
      throw new Error('failed to get cluster nodes: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getVoteAccounts(commitment?: Commitment): Promise<VoteAccountStatus> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getVoteAccounts', args);
    const res = create(unsafeRes, jsonRpcResult(VoteAccountStatus));
    if ('error' in res) {
      throw new Error('failed to get vote accounts: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current slot that the node is processing
   */
  async getSlot(commitment?: Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSlot', args);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if ('error' in res) {
      throw new Error('failed to get slot: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current slot leader of the cluster
   */
  async getSlotLeader(commitment?: Commitment): Promise<string> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSlotLeader', args);
    const res = create(unsafeRes, jsonRpcResult(string()));
    if ('error' in res) {
      throw new Error('failed to get slot leader: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current status of a signature
   */
  async getSignatureStatus(
    signature: TransactionSignature,
    config?: SignatureStatusConfig,
  ): Promise<RpcResponseAndContext<SignatureStatus | null>> {
    const {context, value: values} = await this.getSignatureStatuses(
      [signature],
      config,
    );
    assert(values.length === 1);
    const value = values[0];
    assert(value !== undefined);
    return {context, value};
  }

  /**
   * Fetch the current statuses of a batch of signatures
   */
  async getSignatureStatuses(
    signatures: Array<TransactionSignature>,
    config?: SignatureStatusConfig,
  ): Promise<
    RpcResponseAndContext<Array<SignatureStatus | null>>
  > {
    const params: any[] = [signatures];
    if (config) {
      params.push(config);
    }
    const response = await this._rpcRequest('getSignatureStatuses', params);
    assertType(
      response,
      jsonRpcResultAndContext(array(nullable(SignatureStatus))),
    );
    if ('error' in response) {
      throw new Error(
        'failed to get signature status: ' + response.error.message,
      );
    }
    return response.result;
  }

  /**
   * Fetch the current transaction count of the cluster
   */
  async getTransactionCount(commitment?: Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const res = await this._rpcRequest('getTransactionCount', args);
    assertType(res, jsonRpcResult(number()));
    if ('error' in res) {
      throw new Error(
        'failed to get transaction count: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch the current total currency supply of the cluster in lamports
   */
  async getTotalSupply(commitment?: Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const res = await this._rpcRequest('getTotalSupply', args);
    assertType(res, jsonRpcResult(number()));
    if ('error' in res) {
      throw new Error('failed to get total supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the cluster InflationGovernor parameters
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getInflationGovernor(
    commitment?: Commitment,
  ): Promise<InflationGovernor> {
    const args = this._buildArgs([], commitment);
    const res = await this._rpcRequest('getInflationGovernor', args);
    assertType(res, jsonRpcResult(InflationGovernor));
    if ('error' in res) {
      throw new Error('failed to get inflation: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the Epoch Info parameters
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getEpochInfo(commitment?: Commitment): Promise<EpochInfo> {
    const args = this._buildArgs([], commitment);
    const res = await this._rpcRequest('getEpochInfo', args);
    assertType(res, jsonRpcResult(EpochInfo));
    if ('error' in res) {
      throw new Error('failed to get epoch info: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the Epoch Schedule parameters
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getEpochSchedule(): Promise<EpochSchedule> {
    const res = await this._rpcRequest('getEpochSchedule', []);
    assertType(res, jsonRpcResult(EpochSchedule));
    if ('error' in res) {
      throw new Error('failed to get epoch schedule: ' + res.error.message);
    }
    return res.result;
  }

  // /**
  //  * Fetch the leader schedule for the current epoch
  //  * @return {Promise<RpcResponseAndContext<LeaderSchedule>>}
  //  */
  // async getLeaderSchedule(): Promise<LeaderSchedule> {
  //   const unsafeRes = await this._rpcRequest('getLeaderSchedule', []);
  //   const res = GetLeaderScheduleRpcResult(unsafeRes);
  //   if ('error' in res) {
  //     throw new Error('failed to get leader schedule: ' + res.error.message);
  //   }
  //   
  //   return res.result;
  // }

  /**
   * Fetch the minimum balance needed to exempt an account of `dataLength`
   * size from rent
   * @param dataLength - size of account data
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getMinimumBalanceForRentExemption(
    dataLength: number,
    commitment?: Commitment,
  ): Promise<number> {
    const args = this._buildArgs([dataLength], commitment);
    const res = await this._rpcRequest(
      'getMinimumBalanceForRentExemption',
      args,
    );
    assertType(res, jsonRpcResult(number()));
    if ('error' in res) {
      console.warn('Unable to fetch minimum balance for rent exemption');
      return 0;
    }
    return res.result;
  }

  /**
   * Fetch a recent blockhash from the cluster, return with context
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getRecentBlockhashAndContext(
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<RecentBlockhash>> {
    const args = this._buildArgs([], commitment);
    const res = await this._rpcRequest('getRecentBlockhash', args);
    assertType(res, jsonRpcResultAndContext(RecentBlockhash));
    if ('error' in res) {
      throw new Error('failed to get recent blockhash: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch recent performance samples
   * @param limit - optional limit of samples returned, defaults to 720.
   */
  async getRecentPerformanceSamples(
    limit?: number,
  ): Promise<Array<PerfSample>> {
    const args = this._buildArgs(limit ? [limit] : []);
    const res = await this._rpcRequest('getRecentPerformanceSamples', args);

    assertType(res, jsonRpcResult(array(PerfSample)));
    if ('error' in res) {
      throw new Error(
        'failed to get recent performance samples: ' + res.error.message,
      );
    }

    return res.result;
  }

  /**
   * Fetch the fee calculator for a recent blockhash from the cluster, return with context
   * @param blockhash - query blockhash as a Base58 encoded string
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getFeeCalculatorForBlockhash(
    blockhash: Blockhash,
    commitment?: Commitment,
  ): Promise<RpcResponseAndContext<FeeCalculator | null>> {
    const args = this._buildArgs([blockhash], commitment);
    const res = await this._rpcRequest('getFeeCalculatorForBlockhash', args);

    assertType(res, jsonRpcResultAndContext(GetFeeCalculatorForBlockhash));
    if ('error' in res) {
      throw new Error('failed to get fee calculator: ' + res.error.message);
    }
    const {context, value} = res.result;
    return {
      context,
      value: value && value.feeCalculator,
    };
  }

  /**
   * Fetch a recent blockhash from the cluster
   * @param commitment - optional commitment level, defaults to class commitment level
   */
  async getRecentBlockhash(
    commitment?: Commitment,
  ): Promise<{blockhash: Blockhash; feeCalculator: FeeCalculator}> {
    try {
      const res = await this.getRecentBlockhashAndContext(commitment);
      return res.value;
    } catch (e) {
      throw new Error('failed to get recent blockhash: ' + e);
    }
  }

  /**
   * Fetch the node version
   */
  async getVersion(): Promise<Version> {
    const unsafeRes = await this._rpcRequest('getVersion', []);
    const res = create(unsafeRes, jsonRpcResult(Version));
    if ('error' in res) {
      throw new Error('failed to get version: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch a list of Transactions and transaction statuses from the cluster
   * for a confirmed block
   */
  async getConfirmedBlock(slot: number): Promise<ConfirmedBlock> {
    const unsafeRes = await this._rpcRequest('getConfirmedBlock', [slot]);
    const res = create(unsafeRes, jsonRpcResult(nullable(ConfirmedBlock)));
    if ('error' in res) {
      throw new Error('failed to get confirmed block: ' + res.error.message);
    }
    const result = res.result;
    assert(typeof result !== 'undefined');
    if (!result) {
      throw new Error('Confirmed block ' + slot + ' not found');
    }
    return {
      blockhash: new PublicKey(result.blockhash).toString(),
      previousBlockhash: new PublicKey(result.previousBlockhash).toString(),
      parentSlot: result.parentSlot,
      transactions: result.transactions,
      rewards: result.rewards || [],
    };
  }

  // /**
  //  * Fetch a transaction details for a confirmed transaction
  //  */
  // async getConfirmedTransaction(
  //   signature: TransactionSignature,
  // ): Promise<ConfirmedTransaction | null> {
  //   const unsafeRes = await this._rpcRequest('getConfirmedTransaction', [
  //     signature,
  //   ]);
  //   const {result, error} = GetConfirmedTransactionRpcResult(unsafeRes);
  //   if (error) {
  //     throw new Error('failed to get confirmed transaction: ' + error.message);
  //   }
  //   assert(typeof result !== 'undefined');
  //   if (result === null) {
  //     return result;
  //   }

  //   const {message, signatures} = result.transaction;
  //   return {
  //     slot: result.slot,
  //     transaction: Transaction.populate(new Message(message), signatures),
  //     meta: result.meta,
  //   };
  // }

  // /**
  //  * Fetch parsed transaction details for a confirmed transaction
  //  */
  // async getParsedConfirmedTransaction(
  //   signature: TransactionSignature,
  // ): Promise<ParsedConfirmedTransaction | null> {
  //   const unsafeRes = await this._rpcRequest('getConfirmedTransaction', [
  //     signature,
  //     'jsonParsed',
  //   ]);
  //   const {result, error} = GetParsedConfirmedTransactionRpcResult(unsafeRes);
  //   if (error) {
  //     throw new Error('failed to get confirmed transaction: ' + error.message);
  //   }
  //   assert(typeof result !== 'undefined');
  //   if (result === null) return result;

  //   if (result.meta.innerInstructions) {
  //     result.meta.innerInstructions.forEach(inner => {
  //       inner.instructions.forEach(ix => {
  //         ix.programId = new PublicKey(ix.programId);

  //         if (ix.accounts) {
  //           ix.accounts = ix.accounts.map(account => new PublicKey(account));
  //         }
  //       });
  //     });
  //   }

  //   const {
  //     accountKeys,
  //     instructions,
  //     recentBlockhash,
  //   } = result.transaction.message;
  //   return {
  //     slot: result.slot,
  //     meta: result.meta,
  //     transaction: {
  //       signatures: result.transaction.signatures,
  //       message: {
  //         accountKeys: accountKeys.map(accountKey => ({
  //           pubkey: new PublicKey(accountKey.pubkey),
  //           signer: accountKey.signer,
  //           writable: accountKey.writable,
  //         })),
  //         instructions: instructions.map(ix => {
  //           let mapped: any = {programId: new PublicKey(ix.programId)};
  //           if ('accounts' in ix) {
  //             mapped.accounts = ix.accounts.map(key => new PublicKey(key));
  //           }

  //           return {
  //             ...ix,
  //             ...mapped,
  //           };
  //         }),
  //         recentBlockhash,
  //       },
  //     },
  //   };
  // }

  // /**
  //  * Fetch a list of all the confirmed signatures for transactions involving an address
  //  * within a specified slot range. Max range allowed is 10,000 slots.
  //  *
  //  * @param address queried address
  //  * @param startSlot start slot, inclusive
  //  * @param endSlot end slot, inclusive
  //  */
  // async getConfirmedSignaturesForAddress(
  //   address: PublicKey,
  //   startSlot: number,
  //   endSlot: number,
  // ): Promise<Array<TransactionSignature>> {
  //   const unsafeRes = await this._rpcRequest(
  //     'getConfirmedSignaturesForAddress',
  //     [address.toBase58(), startSlot, endSlot],
  //   );
  //   const result = GetConfirmedSignaturesForAddressRpcResult(unsafeRes);
  //   if (result.error) {
  //     throw new Error(
  //       'failed to get confirmed signatures for address: ' +
  //         result.error.message,
  //     );
  //   }
  //   assert(typeof result.result !== 'undefined');
  //   return result.result;
  // }

  // /**
  //  * Returns confirmed signatures for transactions involving an
  //  * address backwards in time from the provided signature or most recent confirmed block
  //  *
  //  *
  //  * @param address queried address
  //  * @param options
  //  */
  // async getConfirmedSignaturesForAddress2(
  //   address: PublicKey,
  //   options: ?ConfirmedSignaturesForAddress2Options,
  // ): Promise<Array<ConfirmedSignatureInfo>> {
  //   const unsafeRes = await this._rpcRequest(
  //     'getConfirmedSignaturesForAddress2',
  //     [address.toBase58(), options],
  //   );
  //   const result = GetConfirmedSignaturesForAddress2RpcResult(unsafeRes);
  //   if (result.error) {
  //     throw new Error(
  //       'failed to get confirmed signatures for address: ' +
  //         result.error.message,
  //     );
  //   }
  //   assert(typeof result.result !== 'undefined');
  //   return result.result;
  // }

  // /**
  //  * Fetch the contents of a Nonce account from the cluster, return with context
  //  */
  // async getNonceAndContext(
  //   nonceAccount: PublicKey,
  //   commitment?: Commitment,
  // ): Promise<RpcResponseAndContext<NonceAccount | null>> {
  //   const {context, value: accountInfo} = await this.getAccountInfoAndContext(
  //     nonceAccount,
  //     commitment,
  //   );

  //   let value = null;
  //   if (accountInfo !== null) {
  //     value = NonceAccount.fromAccountData(accountInfo.data);
  //   }

  //   return {
  //     context,
  //     value,
  //   };
  // }

  // /**
  //  * Fetch the contents of a Nonce account from the cluster
  //  */
  // async getNonce(
  //   nonceAccount: PublicKey,
  //   commitment?: Commitment,
  // ): Promise<NonceAccount | null> {
  //   return await this.getNonceAndContext(nonceAccount, commitment)
  //     .then(x => x.value)
  //     .catch(e => {
  //       throw new Error(
  //         'failed to get nonce for account ' +
  //           nonceAccount.toBase58() +
  //           ': ' +
  //           e,
  //       );
  //     });
  // }

  /**
   * Request an allocation of lamports to the specified account
   */
  async requestAirdrop(
    to: PublicKey,
    amount: number,
  ): Promise<TransactionSignature> {
    const res = await this._rpcRequest('requestAirdrop', [
      to.toBase58(),
      amount,
    ]);
    assertType(res, jsonRpcResult(string()));
    if ('error' in res) {
      throw new Error(
        'airdrop to ' + to.toBase58() + ' failed: ' + res.error.message,
      );
    }
    return res.result;
  }

  // async _recentBlockhash(disableCache: boolean): Promise<Blockhash> {
  //   if (!disableCache) {
  //     // Wait for polling to finish
  //     while (this._pollingBlockhash) {
  //       await sleep(100);
  //     }
  //     // Attempt to use a recent blockhash for up to 30 seconds
  //     const expired =
  //       Date.now() - this._blockhashInfo.lastFetch >=
  //       BLOCKHASH_CACHE_TIMEOUT_MS;
  //     if (this._blockhashInfo.recentBlockhash !== null && !expired) {
  //       return this._blockhashInfo.recentBlockhash;
  //     }
  //   }

  //   return await this._pollNewBlockhash();
  // }

  // async _pollNewBlockhash(): Promise<Blockhash> {
  //   this._pollingBlockhash = true;
  //   try {
  //     const startTime = Date.now();
  //     for (let i = 0; i < 50; i++) {
  //       const {blockhash} = await this.getRecentBlockhash('max');

  //       if (this._blockhashInfo.recentBlockhash != blockhash) {
  //         this._blockhashInfo = {
  //           recentBlockhash: blockhash,
  //           lastFetch: new Date(),
  //           transactionSignatures: [],
  //           simulatedSignatures: [],
  //         };
  //         return blockhash;
  //       }

  //       // Sleep for approximately half a slot
  //       await sleep(MS_PER_SLOT / 2);
  //     }

  //     throw new Error(
  //       `Unable to obtain a new blockhash after ${Date.now() - startTime}ms`,
  //     );
  //   } finally {
  //     this._pollingBlockhash = false;
  //   }
  // }

  // /**
  //  * Simulate a transaction
  //  */
  // async simulateTransaction(
  //   transaction: Transaction,
  //   signers?: Array<Account>,
  // ): Promise<RpcResponseAndContext<SimulatedTransactionResponse>> {
  //   if (transaction.nonceInfo && signers) {
  //     transaction.sign(...signers);
  //   } else {
  //     let disableCache = this._disableBlockhashCaching;
  //     for (;;) {
  //       transaction.recentBlockhash = await this._recentBlockhash(disableCache);

  //       if (!signers) break;

  //       transaction.sign(...signers);
  //       if (!transaction.signature) {
  //         throw new Error('!signature'); // should never happen
  //       }

  //       // If the signature of this transaction has not been seen before with the
  //       // current recentBlockhash, all done.
  //       const signature = transaction.signature.toString('base64');
  //       if (
  //         !this._blockhashInfo.simulatedSignatures.includes(signature) &&
  //         !this._blockhashInfo.transactionSignatures.includes(signature)
  //       ) {
  //         this._blockhashInfo.simulatedSignatures.push(signature);
  //         break;
  //       } else {
  //         disableCache = true;
  //       }
  //     }
  //   }

  //   const signData = transaction.serializeMessage();
  //   const wireTransaction = transaction._serialize(signData);
  //   const encodedTransaction = wireTransaction.toString('base64');
  //   const config: any = {
  //     encoding: 'base64',
  //     commitment: this.commitment,
  //   };
  //   const args = [encodedTransaction, config];

  //   if (signers) {
  //     config.sigVerify = true;
  //   }

  //   const unsafeRes = await this._rpcRequest('simulateTransaction', args);
  //   const res = SimulatedTransactionResponseValidator(unsafeRes);
  //   if ('error' in res) {
  //     throw new Error('failed to simulate transaction: ' + res.error.message);
  //   }
  //   
  //   assert(res.result);
  //   return res.result;
  // }

  // /**
  //  * Sign and send a transaction
  //  */
  // async sendTransaction(
  //   transaction: Transaction,
  //   signers: Array<Account>,
  //   options?: SendOptions,
  // ): Promise<TransactionSignature> {
  //   if (transaction.nonceInfo) {
  //     transaction.sign(...signers);
  //   } else {
  //     let disableCache = this._disableBlockhashCaching;
  //     for (;;) {
  //       transaction.recentBlockhash = await this._recentBlockhash(disableCache);
  //       transaction.sign(...signers);
  //       if (!transaction.signature) {
  //         throw new Error('!signature'); // should never happen
  //       }

  //       // If the signature of this transaction has not been seen before with the
  //       // current recentBlockhash, all done.
  //       const signature = transaction.signature.toString('base64');
  //       if (!this._blockhashInfo.transactionSignatures.includes(signature)) {
  //         this._blockhashInfo.transactionSignatures.push(signature);
  //         break;
  //       } else {
  //         disableCache = true;
  //       }
  //     }
  //   }

  //   const wireTransaction = transaction.serialize();
  //   return await this.sendRawTransaction(wireTransaction, options);
  // }

  // /**
  //  * @private
  //  */
  // async validatorExit(): Promise<boolean> {
  //   const unsafeRes = await this._rpcRequest('validatorExit', []);
  //   const res = jsonRpcResult(boolean())(unsafeRes);
  //   if ('error' in res) {
  //     throw new Error('validator exit failed: ' + res.error.message);
  //   }
  //   
  //   return res.result;
  // }

  // /**
  //  * Send a transaction that has already been signed and serialized into the
  //  * wire format
  //  */
  // async sendRawTransaction(
  //   rawTransaction: Buffer | Uint8Array | Array<number>,
  //   options: ?SendOptions,
  // ): Promise<TransactionSignature> {
  //   const encodedTransaction = toBuffer(rawTransaction).toString('base64');
  //   const result = await this.sendEncodedTransaction(
  //     encodedTransaction,
  //     options,
  //   );
  //   return result;
  // }

  // /**
  //  * Send a transaction that has already been signed, serialized into the
  //  * wire format, and encoded as a base64 string
  //  */
  // async sendEncodedTransaction(
  //   encodedTransaction: string,
  //   options: ?SendOptions,
  // ): Promise<TransactionSignature> {
  //   const config: any = {encoding: 'base64'};
  //   const args = [encodedTransaction, config];
  //   const skipPreflight = options && options.skipPreflight;
  //   const preflightCommitment =
  //     (options && options.preflightCommitment) || this.commitment;

  //   if (skipPreflight) {
  //     config.skipPreflight = skipPreflight;
  //   }
  //   if (preflightCommitment) {
  //     config.preflightCommitment = preflightCommitment;
  //   }

  //   const unsafeRes = await this._rpcRequest('sendTransaction', args);
  //   const res = SendTransactionRpcResult(unsafeRes);
  //   if ('error' in res) {
  //     if ('error' in res.data) {
  //       const logs = res.error.data.logs;
  //       if (logs && Array.isArray(logs)) {
  //         const traceIndent = '\n    ';
  //         const logTrace = traceIndent + logs.join(traceIndent);
  //         console.error(res.error.message, logTrace);
  //       }
  //     }
  //     throw new Error('failed to send transaction: ' + res.error.message);
  //   }
  //   
  //   assert(res.result);
  //   return res.result;
  // }

  // /**
  //  * @private
  //  */
  // _wsOnOpen() {
  //   this._rpcWebSocketConnected = true;
  //   this._rpcWebSocketHeartbeat = setInterval(() => {
  //     // Ping server every 5s to prevent idle timeouts
  //     this._rpcWebSocket.notify('ping').catch(() => {});
  //   }, 5000);
  //   this._updateSubscriptions();
  // }

  // /**
  //  * @private
  //  */
  // _wsOnError(err: Error) {
  //   console.error('ws error:', err.message);
  // }

  // /**
  //  * @private
  //  */
  // _wsOnClose(code: number) {
  //   clearInterval(this._rpcWebSocketHeartbeat);
  //   this._rpcWebSocketHeartbeat = null;

  //   if (code === 1000) {
  //     // explicit close, check if any subscriptions have been made since close
  //     this._updateSubscriptions();
  //     return;
  //   }

  //   // implicit close, prepare subscriptions for auto-reconnect
  //   this._resetSubscriptions();
  // }

  // /**
  //  * @private
  //  */
  // async _subscribe<SubInfo extends {subscriptionId: ?SubscriptionId}, RpcArgs>(
  //   sub: SubInfo,
  //   rpcMethod: string,
  //   rpcArgs: RpcArgs,
  // ) {
  //   if (sub.subscriptionId == null) {
  //     sub.subscriptionId = 'subscribing';
  //     try {
  //       const id = await this._rpcWebSocket.call(rpcMethod, rpcArgs);
  //       if (sub.subscriptionId === 'subscribing') {
  //         // eslint-disable-next-line require-atomic-updates
  //         sub.subscriptionId = id;
  //       }
  //     } catch (err) {
  //       if (sub.subscriptionId === 'subscribing') {
  //         // eslint-disable-next-line require-atomic-updates
  //         sub.subscriptionId = null;
  //       }
  //       console.error(`${rpcMethod} error for argument`, rpcArgs, err.message);
  //     }
  //   }
  // }

  // /**
  //  * @private
  //  */
  // async _unsubscribe<SubInfo extends {subscriptionId: ?SubscriptionId}>(
  //   sub: SubInfo,
  //   rpcMethod: string,
  // ) {
  //   const subscriptionId = sub.subscriptionId;
  //   if (subscriptionId != null && typeof subscriptionId != string()) {
  //     const unsubscribeId: number = subscriptionId;
  //     try {
  //       await this._rpcWebSocket.call(rpcMethod, [unsubscribeId]);
  //     } catch (err) {
  //       console.error(`${rpcMethod} error:`, err.message);
  //     }
  //   }
  // }

  // /**
  //  * @private
  //  */
  // _resetSubscriptions() {
  //   Object.values(this._accountChangeSubscriptions).forEach(
  //     s => (s.subscriptionId = null),
  //   );
  //   Object.values(this._programAccountChangeSubscriptions).forEach(
  //     s => (s.subscriptionId = null),
  //   );
  //   Object.values(this._signatureSubscriptions).forEach(
  //     s => (s.subscriptionId = null),
  //   );
  //   Object.values(this._slotSubscriptions).forEach(
  //     s => (s.subscriptionId = null),
  //   );
  //   Object.values(this._rootSubscriptions).forEach(
  //     s => (s.subscriptionId = null),
  //   );
  // }

  // /**
  //  * @private
  //  */
  // _updateSubscriptions() {
  //   const accountKeys = Object.keys(this._accountChangeSubscriptions).map(
  //     Number,
  //   );
  //   const programKeys = Object.keys(
  //     this._programAccountChangeSubscriptions,
  //   ).map(Number);
  //   const slotKeys = Object.keys(this._slotSubscriptions).map(Number);
  //   const signatureKeys = Object.keys(this._signatureSubscriptions).map(Number);
  //   const rootKeys = Object.keys(this._rootSubscriptions).map(Number);
  //   if (
  //     accountKeys.length === 0 &&
  //     programKeys.length === 0 &&
  //     slotKeys.length === 0 &&
  //     signatureKeys.length === 0 &&
  //     rootKeys.length === 0
  //   ) {
  //     if (this._rpcWebSocketConnected) {
  //       this._rpcWebSocketConnected = false;
  //       this._rpcWebSocketIdleTimeout = setTimeout(() => {
  //         this._rpcWebSocketIdleTimeout = null;
  //         this._rpcWebSocket.close();
  //       }, 500);
  //     }
  //     return;
  //   }

  //   if (this._rpcWebSocketIdleTimeout !== null) {
  //     clearTimeout(this._rpcWebSocketIdleTimeout);
  //     this._rpcWebSocketIdleTimeout = null;
  //     this._rpcWebSocketConnected = true;
  //   }

  //   if (!this._rpcWebSocketConnected) {
  //     this._rpcWebSocket.connect();
  //     return;
  //   }

  //   for (let id of accountKeys) {
  //     const sub = this._accountChangeSubscriptions[id];
  //     this._subscribe(
  //       sub,
  //       'accountSubscribe',
  //       this._buildArgs([sub.publicKey], sub.commitment, 'base64'),
  //     );
  //   }

  //   for (let id of programKeys) {
  //     const sub = this._programAccountChangeSubscriptions[id];
  //     this._subscribe(
  //       sub,
  //       'programSubscribe',
  //       this._buildArgs([sub.programId], sub.commitment, 'base64'),
  //     );
  //   }

  //   for (let id of slotKeys) {
  //     const sub = this._slotSubscriptions[id];
  //     this._subscribe(sub, 'slotSubscribe', []);
  //   }

  //   for (let id of signatureKeys) {
  //     const sub = this._signatureSubscriptions[id];
  //     this._subscribe(
  //       sub,
  //       'signatureSubscribe',
  //       this._buildArgs([sub.signature], sub.commitment),
  //     );
  //   }

  //   for (let id of rootKeys) {
  //     const sub = this._rootSubscriptions[id];
  //     this._subscribe(sub, 'rootSubscribe', []);
  //   }
  // }

  // /**
  //  * @private
  //  */
  // _wsOnAccountNotification(notification: Object) {
  //   const res = AccountNotificationResult(notification);
  //   if ('error' in res) {
  //     throw new Error('account notification failed: ' + res.error.message);
  //   }
  //   
  //   const keys = Object.keys(this._accountChangeSubscriptions).map(Number);
  //   for (let id of keys) {
  //     const sub = this._accountChangeSubscriptions[id];
  //     if (sub.subscriptionId === res.subscription) {
  //       const {result} = res;
  //       const {value, context} = result;

  //       assert(value.data[1] === 'base64');
  //       sub.callback(
  //         {
  //           executable: value.executable,
  //           owner: new PublicKey(value.owner),
  //           lamports: value.lamports,
  //           data: Buffer.from(value.data[0], 'base64'),
  //         },
  //         context,
  //       );
  //       return true;
  //     }
  //   }
  // }

  // /**
  //  * Register a callback to be invoked whenever the specified account changes
  //  *
  //  * @param publicKey Public key of the account to monitor
  //  * @param callback Function to invoke whenever the account is changed
  //  * @param commitment Specify the commitment level account changes must reach before notification
  //  * @return subscription id
  //  */
  // onAccountChange(
  //   publicKey: PublicKey,
  //   callback: AccountChangeCallback,
  //   commitment?: Commitment,
  // ): number {
  //   const id = ++this._accountChangeSubscriptionCounter;
  //   this._accountChangeSubscriptions[id] = {
  //     publicKey: publicKey.toBase58(),
  //     callback,
  //     commitment,
  //     subscriptionId: null,
  //   };
  //   this._updateSubscriptions();
  //   return id;
  // }

  // /**
  //  * Deregister an account notification callback
  //  *
  //  * @param id subscription id to deregister
  //  */
  // async removeAccountChangeListener(id: number): Promise<void> {
  //   if (this._accountChangeSubscriptions[id]) {
  //     const subInfo = this._accountChangeSubscriptions[id];
  //     delete this._accountChangeSubscriptions[id];
  //     await this._unsubscribe(subInfo, 'accountUnsubscribe');
  //     this._updateSubscriptions();
  //   } else {
  //     throw new Error(`Unknown account change id: ${id}`);
  //   }
  // }

  // /**
  //  * @private
  //  */
  // _wsOnProgramAccountNotification(notification: Object) {
  //   const res = ProgramAccountNotificationResult(notification);
  //   if ('error' in res) {
  //     throw new Error(
  //       'program account notification failed: ' + res.error.message,
  //     );
  //   }
  //   
  //   const keys = Object.keys(this._programAccountChangeSubscriptions).map(
  //     Number,
  //   );
  //   for (let id of keys) {
  //     const sub = this._programAccountChangeSubscriptions[id];
  //     if (sub.subscriptionId === res.subscription) {
  //       const {result} = res;
  //       const {value, context} = result;

  //       assert(value.account.data[1] === 'base64');
  //       sub.callback(
  //         {
  //           accountId: value.pubkey,
  //           accountInfo: {
  //             executable: value.account.executable,
  //             owner: new PublicKey(value.account.owner),
  //             lamports: value.account.lamports,
  //             data: Buffer.from(value.account.data[0], 'base64'),
  //           },
  //         },
  //         context,
  //       );
  //       return true;
  //     }
  //   }
  // }

  // /**
  //  * Register a callback to be invoked whenever accounts owned by the
  //  * specified program change
  //  *
  //  * @param programId Public key of the program to monitor
  //  * @param callback Function to invoke whenever the account is changed
  //  * @param commitment Specify the commitment level account changes must reach before notification
  //  * @return subscription id
  //  */
  // onProgramAccountChange(
  //   programId: PublicKey,
  //   callback: ProgramAccountChangeCallback,
  //   commitment?: Commitment,
  // ): number {
  //   const id = ++this._programAccountChangeSubscriptionCounter;
  //   this._programAccountChangeSubscriptions[id] = {
  //     programId: programId.toBase58(),
  //     callback,
  //     commitment,
  //     subscriptionId: null,
  //   };
  //   this._updateSubscriptions();
  //   return id;
  // }

  // /**
  //  * Deregister an account notification callback
  //  *
  //  * @param id subscription id to deregister
  //  */
  // async removeProgramAccountChangeListener(id: number): Promise<void> {
  //   if (this._programAccountChangeSubscriptions[id]) {
  //     const subInfo = this._programAccountChangeSubscriptions[id];
  //     delete this._programAccountChangeSubscriptions[id];
  //     await this._unsubscribe(subInfo, 'programUnsubscribe');
  //     this._updateSubscriptions();
  //   } else {
  //     throw new Error(`Unknown program account change id: ${id}`);
  //   }
  // }

  // /**
  //  * @private
  //  */
  // _wsOnSlotNotification(notification: Object) {
  //   const res = SlotNotificationResult(notification);
  //   if ('error' in res) {
  //     throw new Error('slot notification failed: ' + res.error.message);
  //   }
  //   
  //   const {parent, slot, root} = res.result;
  //   const keys = Object.keys(this._slotSubscriptions).map(Number);
  //   for (let id of keys) {
  //     const sub = this._slotSubscriptions[id];
  //     if (sub.subscriptionId === res.subscription) {
  //       sub.callback({
  //         parent,
  //         slot,
  //         root,
  //       });
  //       return true;
  //     }
  //   }
  // }

  // /**
  //  * Register a callback to be invoked upon slot changes
  //  *
  //  * @param callback Function to invoke whenever the slot changes
  //  * @return subscription id
  //  */
  // onSlotChange(callback: SlotChangeCallback): number {
  //   const id = ++this._slotSubscriptionCounter;
  //   this._slotSubscriptions[id] = {
  //     callback,
  //     subscriptionId: null,
  //   };
  //   this._updateSubscriptions();
  //   return id;
  // }

  // /**
  //  * Deregister a slot notification callback
  //  *
  //  * @param id subscription id to deregister
  //  */
  // async removeSlotChangeListener(id: number): Promise<void> {
  //   if (this._slotSubscriptions[id]) {
  //     const subInfo = this._slotSubscriptions[id];
  //     delete this._slotSubscriptions[id];
  //     await this._unsubscribe(subInfo, 'slotUnsubscribe');
  //     this._updateSubscriptions();
  //   } else {
  //     throw new Error(`Unknown slot change id: ${id}`);
  //   }
  // }

  private _buildArgs(
    args: Array<any>,
    override?: Commitment,
    encoding?: 'jsonParsed' | 'base64',
    extra?: any,
  ): Array<any> {
    const commitment = override || this._commitment;
    if (commitment || encoding || extra) {
      let options: any = {};
      if (encoding) {
        options.encoding = encoding;
      }
      if (commitment) {
        options.commitment = commitment;
      }
      if (extra) {
        options = Object.assign(options, extra);
      }
      args.push(options);
    }
    return args;
  }

  // /**
  //  * @private
  //  */
  // _wsOnSignatureNotification(notification: Object) {
  //   const res = SignatureNotificationResult(notification);
  //   if ('error' in res) {
  //     throw new Error('signature notification failed: ' + res.error.message);
  //   }
  //   
  //   const keys = Object.keys(this._signatureSubscriptions).map(Number);
  //   for (let id of keys) {
  //     const sub = this._signatureSubscriptions[id];
  //     if (sub.subscriptionId === res.subscription) {
  //       // Signatures subscriptions are auto-removed by the RPC service so
  //       // no need to explicitly send an unsubscribe message
  //       delete this._signatureSubscriptions[id];
  //       this._updateSubscriptions();
  //       sub.callback(res.result.value, res.result.context);
  //       return;
  //     }
  //   }
  // }

  // /**
  //  * Register a callback to be invoked upon signature updates
  //  *
  //  * @param signature Transaction signature string in base 58
  //  * @param callback Function to invoke on signature notifications
  //  * @param commitment Specify the commitment level signature must reach before notification
  //  * @return subscription id
  //  */
  // onSignature(
  //   signature: TransactionSignature,
  //   callback: SignatureResultCallback,
  //   commitment?: Commitment,
  // ): number {
  //   const id = ++this._signatureSubscriptionCounter;
  //   this._signatureSubscriptions[id] = {
  //     signature,
  //     callback,
  //     commitment,
  //     subscriptionId: null,
  //   };
  //   this._updateSubscriptions();
  //   return id;
  // }

  // /**
  //  * Deregister a signature notification callback
  //  *
  //  * @param id subscription id to deregister
  //  */
  // async removeSignatureListener(id: number): Promise<void> {
  //   if (this._signatureSubscriptions[id]) {
  //     const subInfo = this._signatureSubscriptions[id];
  //     delete this._signatureSubscriptions[id];
  //     await this._unsubscribe(subInfo, 'signatureUnsubscribe');
  //     this._updateSubscriptions();
  //   } else {
  //     throw new Error(`Unknown signature result id: ${id}`);
  //   }
  // }

  // /**
  //  * @private
  //  */
  // _wsOnRootNotification(notification: Object) {
  //   const res = RootNotificationResult(notification);
  //   if ('error' in res) {
  //     throw new Error('root notification failed: ' + res.error.message);
  //   }
  //   
  //   const root = res.result;
  //   const keys = Object.keys(this._rootSubscriptions).map(Number);
  //   for (let id of keys) {
  //     const sub = this._rootSubscriptions[id];
  //     if (sub.subscriptionId === res.subscription) {
  //       sub.callback(root);
  //       return true;
  //     }
  //   }
  // }

  // /**
  //  * Register a callback to be invoked upon root changes
  //  *
  //  * @param callback Function to invoke whenever the root changes
  //  * @return subscription id
  //  */
  // onRootChange(callback: RootChangeCallback): number {
  //   const id = ++this._rootSubscriptionCounter;
  //   this._rootSubscriptions[id] = {
  //     callback,
  //     subscriptionId: null,
  //   };
  //   this._updateSubscriptions();
  //   return id;
  // }

  // /**
  //  * Deregister a root notification callback
  //  *
  //  * @param id subscription id to deregister
  //  */
  // async removeRootChangeListener(id: number): Promise<void> {
  //   if (this._rootSubscriptions[id]) {
  //     const subInfo = this._rootSubscriptions[id];
  //     delete this._rootSubscriptions[id];
  //     await this._unsubscribe(subInfo, 'rootUnsubscribe');
  //     this._updateSubscriptions();
  //   } else {
  //     throw new Error(`Unknown root change id: ${id}`);
  //   }
  // }
}
