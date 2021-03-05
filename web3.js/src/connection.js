// @flow

import assert from 'assert';
import bs58 from 'bs58';
import {Buffer} from 'buffer';
import {parse as urlParse, format as urlFormat} from 'url';
import fetch from 'node-fetch';
import jayson from 'jayson/lib/client/browser';
import {
  type as pick,
  number,
  string,
  array,
  boolean,
  literal,
  record,
  union,
  optional,
  nullable,
  coerce,
  instance,
  create,
  tuple,
  unknown,
} from 'superstruct';
import type {Struct} from 'superstruct';
import {Client as RpcWebSocketClient} from 'rpc-websockets';

import {AgentManager} from './agent-manager';
import {NonceAccount} from './nonce-account';
import {PublicKey} from './publickey';
import {MS_PER_SLOT} from './timing';
import {Transaction} from './transaction';
import {Message} from './message';
import {sleep} from './util/sleep';
import {promiseTimeout} from './util/promise-timeout';
import {toBuffer} from './util/to-buffer';
import type {Blockhash} from './blockhash';
import type {FeeCalculator} from './fee-calculator';
import type {Account} from './account';
import type {TransactionSignature} from './transaction';
import type {CompiledInstruction} from './message';

const PublicKeyFromString = coerce(
  instance(PublicKey),
  string(),
  value => new PublicKey(value),
);

const RawAccountDataResult = tuple([string(), literal('base64')]);

const BufferFromRawAccountData = coerce(
  instance(Buffer),
  RawAccountDataResult,
  value => Buffer.from(value[0], 'base64'),
);

export const BLOCKHASH_CACHE_TIMEOUT_MS = 30 * 1000;

type RpcRequest = (methodName: string, args: Array<any>) => any;

type TokenAccountsFilter =
  | {|
      mint: PublicKey,
    |}
  | {|
      programId: PublicKey,
    |};

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
 * Options for sending transactions
 *
 * @typedef {Object} SendOptions
 * @property {boolean | undefined} skipPreflight disable transaction verification step
 * @property {Commitment | undefined} preflightCommitment preflight commitment level
 */
export type SendOptions = {
  skipPreflight?: boolean,
  preflightCommitment?: Commitment,
};

/**
 * Options for confirming transactions
 *
 * @typedef {Object} ConfirmOptions
 * @property {boolean | undefined} skipPreflight disable transaction verification step
 * @property {Commitment | undefined} commitment desired commitment level
 * @property {Commitment | undefined} preflightCommitment preflight commitment level
 */
export type ConfirmOptions = {
  skipPreflight?: boolean,
  commitment?: Commitment,
  preflightCommitment?: Commitment,
};

/**
 * Options for getConfirmedSignaturesForAddress2
 *
 * @typedef {Object} ConfirmedSignaturesForAddress2Options
 * @property {TransactionSignature | undefined} before start searching backwards from this transaction signature.
 *               If not provided the search starts from the highest max confirmed block.
 * @property {number | undefined} limit maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
 *
 */
export type ConfirmedSignaturesForAddress2Options = {
  before?: TransactionSignature,
  limit?: number,
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
function createRpcResult<T, U>(result: Struct<T, U>) {
  return union([
    pick({
      jsonrpc: literal('2.0'),
      id: string(),
      result,
    }),
    pick({
      jsonrpc: literal('2.0'),
      id: string(),
      error: pick({
        code: unknown(),
        message: string(),
        data: optional(unknown()),
      }),
    }),
  ]);
}

const UnknownRpcResult = createRpcResult(unknown());

/**
 * @private
 */
function jsonRpcResult<T, U>(schema: Struct<T, U>) {
  return coerce(createRpcResult(schema), UnknownRpcResult, value => {
    if ('error' in value) {
      return value;
    } else {
      return {
        ...value,
        result: create(value.result, schema),
      };
    }
  });
}

/**
 * @private
 */
function jsonRpcResultAndContext<T, U>(value: Struct<T, U>) {
  return jsonRpcResult(
    pick({
      context: pick({
        slot: number(),
      }),
      value,
    }),
  );
}

/**
 * @private
 */
function notificationResultAndContext<T, U>(value: Struct<T, U>) {
  return pick({
    context: pick({
      slot: number(),
    }),
    value,
  });
}

/**
 * The level of commitment desired when querying state
 * <pre>
 *   'processed': Query the most recent block which has reached 1 confirmation by the connected node
 *   'confirmed': Query the most recent block which has reached 1 confirmation by the cluster
 *   'finalized': Query the most recent block which has been finalized by the cluster
 * </pre>
 *
 * @typedef {'processed' | 'confirmed' | 'finalized'} Commitment
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
 * <pre>
 *   'circulating':    Return the largest accounts that are part of the circulating supply
 *   'nonCirculating': Return the largest accounts that are not part of the circulating supply
 * </pre>
 *
 * @typedef {'circulating' | 'nonCirculating'} LargestAccountsFilter
 */
export type LargestAccountsFilter = 'circulating' | 'nonCirculating';

/**
 * Configuration object for changing `getLargestAccounts` query behavior
 *
 * @typedef {Object} GetLargestAccountsConfig
 * @property {Commitment|undefined} commitment The level of commitment desired
 * @property {LargestAccountsFilter|undefined} filter Filter largest accounts by whether they are part of the circulating supply
 */
type GetLargestAccountsConfig = {
  commitment: ?Commitment,
  filter: ?LargestAccountsFilter,
};

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
 * @property {string|null} gossip Gossip network address for the node
 * @property {string|null} tpu TPU network address for the node (null if not available)
 * @property {string|null} rpc JSON RPC network address for the node (null if not available)
 * @property {string|null} version Software version of the node (null if not available)
 */
type ContactInfo = {
  pubkey: string,
  gossip: string | null,
  tpu: string | null,
  rpc: string | null,
  version: string | null,
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
 * Network Inflation
 * (see https://docs.solana.com/implemented-proposals/ed_overview)
 *
 * @typedef {Object} InflationGovernor
 * @property {number} foundation
 * @property {number} foundation_term
 * @property {number} initial
 * @property {number} taper
 * @property {number} terminal
 */
type InflationGovernor = {
  foundation: number,
  foundationTerm: number,
  initial: number,
  taper: number,
  terminal: number,
};

const GetInflationGovernorResult = pick({
  foundation: number(),
  foundationTerm: number(),
  initial: number(),
  taper: number(),
  terminal: number(),
});

/**
 * Information about the current epoch
 *
 * @typedef {Object} EpochInfo
 * @property {number} epoch
 * @property {number} slotIndex
 * @property {number} slotsInEpoch
 * @property {number} absoluteSlot
 * @property {number} blockHeight
 * @property {number} transactionCount
 */
type EpochInfo = {
  epoch: number,
  slotIndex: number,
  slotsInEpoch: number,
  absoluteSlot: number,
  blockHeight: number | null,
  transactionCount: number | null,
};

const GetEpochInfoResult = pick({
  epoch: number(),
  slotIndex: number(),
  slotsInEpoch: number(),
  absoluteSlot: number(),
  blockHeight: optional(number()),
  transactionCount: optional(number()),
});

/**
 * Epoch schedule
 * (see https://docs.solana.com/terminology#epoch)
 *
 * @typedef {Object} EpochSchedule
 * @property {number} slotsPerEpoch The maximum number of slots in each epoch
 * @property {number} leaderScheduleSlotOffset The number of slots before beginning of an epoch to calculate a leader schedule for that epoch
 * @property {boolean} warmup Indicates whether epochs start short and grow
 * @property {number} firstNormalEpoch The first epoch with `slotsPerEpoch` slots
 * @property {number} firstNormalSlot The first slot of `firstNormalEpoch`
 */
type EpochSchedule = {
  slotsPerEpoch: number,
  leaderScheduleSlotOffset: number,
  warmup: boolean,
  firstNormalEpoch: number,
  firstNormalSlot: number,
};

const GetEpochScheduleResult = pick({
  slotsPerEpoch: number(),
  leaderScheduleSlotOffset: number(),
  warmup: boolean(),
  firstNormalEpoch: number(),
  firstNormalSlot: number(),
});

/**
 * Leader schedule
 * (see https://docs.solana.com/terminology#leader-schedule)
 *
 * @typedef {Object} LeaderSchedule
 */
type LeaderSchedule = {
  [address: string]: number[],
};

// TODO: check if validating array(number()) is still extremely slow
const GetLeaderScheduleResult = record(string(), unknown());

/**
 * Transaction error or null
 */
const TransactionErrorResult = nullable(pick({}));

/**
 * Signature status for a transaction
 */
const SignatureStatusResult = pick({
  err: TransactionErrorResult,
});

/**
 * Version info for a node
 *
 * @typedef {Object} Version
 * @property {string} solana-core Version of solana-core
 */
const Version = pick({
  'solana-core': string(),
  'feature-set': optional(nullable(number())),
});

type SimulatedTransactionResponse = {
  err: TransactionError | string | null,
  logs: Array<string> | null,
};

const SimulatedTransactionResponseStruct = jsonRpcResultAndContext(
  pick({
    err: nullable(union([pick({}), string()])),
    logs: nullable(array(string())),
  }),
);

type ParsedInnerInstruction = {
  index: number,
  instructions: (ParsedInstruction | PartiallyDecodedInstruction)[],
};

type TokenBalance = {
  accountIndex: number,
  mint: string,
  uiTokenAmount: TokenAmount,
};

/**
 * Metadata for a parsed confirmed transaction on the ledger
 *
 * @typedef {Object} ParsedConfirmedTransactionMeta
 * @property {number} fee The fee charged for processing the transaction
 * @property {Array<ParsedInnerInstruction>} innerInstructions An array of cross program invoked parsed instructions
 * @property {Array<number>} preBalances The balances of the transaction accounts before processing
 * @property {Array<number>} postBalances The balances of the transaction accounts after processing
 * @property {Array<string>} logMessages An array of program log messages emitted during a transaction
 * @property {Array<TokenBalance>} preTokenBalances The token balances of the transaction accounts before processing
 * @property {Array<TokenBalance>} postTokenBalances The token balances of the transaction accounts after processing
 * @property {object|null} err The error result of transaction processing
 */
type ParsedConfirmedTransactionMeta = {
  fee: number,
  innerInstructions?: ParsedInnerInstruction[],
  preBalances: Array<number>,
  postBalances: Array<number>,
  logMessages?: Array<string>,
  preTokenBalances?: Array<TokenBalance>,
  postTokenBalances?: Array<TokenBalance>,
  err: TransactionError | null,
};

type CompiledInnerInstruction = {
  index: number,
  instructions: CompiledInstruction[],
};

/**
 * Metadata for a confirmed transaction on the ledger
 *
 * @typedef {Object} ConfirmedTransactionMeta
 * @property {number} fee The fee charged for processing the transaction
 * @property {Array<CompiledInnerInstruction>} innerInstructions An array of cross program invoked instructions
 * @property {Array<number>} preBalances The balances of the transaction accounts before processing
 * @property {Array<number>} postBalances The balances of the transaction accounts after processing
 * @property {Array<string>} logMessages An array of program log messages emitted during a transaction
 * @property {Array<TokenBalance>} preTokenBalances The token balances of the transaction accounts before processing
 * @property {Array<TokenBalance>} postTokenBalances The token balances of the transaction accounts after processing
 * @property {object|null} err The error result of transaction processing
 */
type ConfirmedTransactionMeta = {
  fee: number,
  innerInstructions?: CompiledInnerInstruction[],
  preBalances: Array<number>,
  postBalances: Array<number>,
  logMessages?: Array<string>,
  preTokenBalances?: Array<TokenBalance>,
  postTokenBalances?: Array<TokenBalance>,
  err: TransactionError | null,
};

/**
 * A confirmed transaction on the ledger
 *
 * @typedef {Object} ConfirmedTransaction
 * @property {number} slot The slot during which the transaction was processed
 * @property {Transaction} transaction The details of the transaction
 * @property {ConfirmedTransactionMeta|null} meta Metadata produced from the transaction
 * @property {number|null|undefined} blockTime The unix timestamp of when the transaction was processed
 */
type ConfirmedTransaction = {
  slot: number,
  transaction: Transaction,
  meta: ConfirmedTransactionMeta | null,
  blockTime?: number | null,
};

/**
 * A partially decoded transaction instruction
 *
 * @typedef {Object} ParsedMessageAccount
 * @property {PublicKey} pubkey Public key of the account
 * @property {PublicKey} accounts Indicates if the account signed the transaction
 * @property {string} data Raw base-58 instruction data
 */
type PartiallyDecodedInstruction = {|
  programId: PublicKey,
  accounts: Array<PublicKey>,
  data: string,
|};

/**
 * A parsed transaction message account
 *
 * @typedef {Object} ParsedMessageAccount
 * @property {PublicKey} pubkey Public key of the account
 * @property {boolean} signer Indicates if the account signed the transaction
 * @property {boolean} writable Indicates if the account is writable for this transaction
 */
type ParsedMessageAccount = {
  pubkey: PublicKey,
  signer: boolean,
  writable: boolean,
};

/**
 * A parsed transaction instruction
 *
 * @typedef {Object} ParsedInstruction
 * @property {string} program Name of the program for this instruction
 * @property {PublicKey} programId ID of the program for this instruction
 * @property {any} parsed Parsed instruction info
 */
type ParsedInstruction = {|
  program: string,
  programId: PublicKey,
  parsed: any,
|};

/**
 * A parsed transaction message
 *
 * @typedef {Object} ParsedMessage
 * @property {Array<ParsedMessageAccount>} accountKeys Accounts used in the instructions
 * @property {Array<ParsedInstruction | PartiallyDecodedInstruction>} instructions The atomically executed instructions for the transaction
 * @property {string} recentBlockhash Recent blockhash
 */
type ParsedMessage = {
  accountKeys: ParsedMessageAccount[],
  instructions: (ParsedInstruction | PartiallyDecodedInstruction)[],
  recentBlockhash: string,
};

/**
 * A parsed transaction
 *
 * @typedef {Object} ParsedTransaction
 * @property {Array<string>} signatures Signatures for the transaction
 * @property {ParsedMessage} message Message of the transaction
 */
type ParsedTransaction = {
  signatures: Array<string>,
  message: ParsedMessage,
};

/**
 * A parsed and confirmed transaction on the ledger
 *
 * @typedef {Object} ParsedConfirmedTransaction
 * @property {number} slot The slot during which the transaction was processed
 * @property {ParsedTransaction} transaction The details of the transaction
 * @property {ConfirmedTransactionMeta|null} meta Metadata produced from the transaction
 * @property {number|null|undefined} blockTime The unix timestamp of when the transaction was processed
 */
type ParsedConfirmedTransaction = {
  slot: number,
  transaction: ParsedTransaction,
  meta: ParsedConfirmedTransactionMeta | null,
  blockTime?: number | null,
};

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
    meta: ConfirmedTransactionMeta | null,
  }>,
  rewards: Array<{
    pubkey: string,
    lamports: number,
    postBalance: number | null,
    rewardType: string | null,
  }>,
};

/**
 * A performance sample
 *
 * @typedef {Object} PerfSample
 * @property {number} slot Slot number of sample
 * @property {number} numTransactions Number of transactions in a sample window
 * @property {number} numSlots Number of slots in a sample window
 * @property {number} samplePeriodSecs Sample window in seconds
 */
type PerfSample = {
  slot: number,
  numTransactions: number,
  numSlots: number,
  samplePeriodSecs: number,
};

function createRpcRequest(url: string, useHttps: boolean): RpcRequest {
  let agentManager;
  if (!process.env.BROWSER) {
    agentManager = new AgentManager(useHttps);
  }

  const server = jayson(async (request, callback) => {
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
      let res = {};
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
 * Expected JSON RPC response for the "getInflationGovernor" message
 */
const GetInflationGovernorRpcResult = jsonRpcResult(GetInflationGovernorResult);

/**
 * Expected JSON RPC response for the "getEpochInfo" message
 */
const GetEpochInfoRpcResult = jsonRpcResult(GetEpochInfoResult);

/**
 * Expected JSON RPC response for the "getEpochSchedule" message
 */
const GetEpochScheduleRpcResult = jsonRpcResult(GetEpochScheduleResult);

/**
 * Expected JSON RPC response for the "getLeaderSchedule" message
 */
const GetLeaderScheduleRpcResult = jsonRpcResult(GetLeaderScheduleResult);

/**
 * Expected JSON RPC response for the "minimumLedgerSlot" and "getFirstAvailableBlock" messages
 */
const SlotRpcResult = jsonRpcResult(number());

/**
 * Supply
 *
 * @typedef {Object} Supply
 * @property {number} total Total supply in lamports
 * @property {number} circulating Circulating supply in lamports
 * @property {number} nonCirculating Non-circulating supply in lamports
 * @property {Array<PublicKey>} nonCirculatingAccounts List of non-circulating account addresses
 */
type Supply = {
  total: number,
  circulating: number,
  nonCirculating: number,
  nonCirculatingAccounts: Array<PublicKey>,
};

/**
 * Expected JSON RPC response for the "getSupply" message
 */
const GetSupplyRpcResult = jsonRpcResultAndContext(
  pick({
    total: number(),
    circulating: number(),
    nonCirculating: number(),
    nonCirculatingAccounts: array(PublicKeyFromString),
  }),
);

/**
 * Token amount object which returns a token amount in different formats
 * for various client use cases.
 *
 * @typedef {Object} TokenAmount
 * @property {string} amount Raw amount of tokens as string ignoring decimals
 * @property {number} decimals Number of decimals configured for token's mint
 * @property {number} uiAmount Token account as float, accounts for decimals
 */
type TokenAmount = {
  amount: string,
  decimals: number,
  uiAmount: number | null,
  uiAmountString?: string,
};

/**
 * Expected JSON RPC structure for token amounts
 */
const TokenAmountResult = pick({
  amount: string(),
  uiAmount: nullable(number()),
  decimals: number(),
  uiAmountString: optional(nullable(string())),
});

/**
 * Token address and balance.
 *
 * @typedef {Object} TokenAccountBalancePair
 * @property {PublicKey} address Address of the token account
 * @property {string} amount Raw amount of tokens as string ignoring decimals
 * @property {number} decimals Number of decimals configured for token's mint
 * @property {number} uiAmount Token account as float, accounts for decimals
 */
type TokenAccountBalancePair = {
  address: PublicKey,
  amount: string,
  decimals: number,
  uiAmount: number,
};

/**
 * Expected JSON RPC response for the "getTokenLargestAccounts" message
 */
const GetTokenLargestAccountsResult = jsonRpcResultAndContext(
  array(
    pick({
      address: PublicKeyFromString,
      amount: string(),
      uiAmount: number(),
      decimals: number(),
    }),
  ),
);

/**
 * Expected JSON RPC response for the "getTokenAccountsByOwner" message
 */
const GetTokenAccountsByOwner = jsonRpcResultAndContext(
  array(
    pick({
      pubkey: PublicKeyFromString,
      account: pick({
        executable: boolean(),
        owner: PublicKeyFromString,
        lamports: number(),
        data: BufferFromRawAccountData,
        rentEpoch: number(),
      }),
    }),
  ),
);

const ParsedAccountDataResult = pick({
  program: string(),
  parsed: unknown(),
  space: number(),
});

/**
 * Expected JSON RPC response for the "getTokenAccountsByOwner" message with parsed data
 */
const GetParsedTokenAccountsByOwner = jsonRpcResultAndContext(
  array(
    pick({
      pubkey: PublicKeyFromString,
      account: pick({
        executable: boolean(),
        owner: PublicKeyFromString,
        lamports: number(),
        data: ParsedAccountDataResult,
        rentEpoch: number(),
      }),
    }),
  ),
);

/**
 * Pair of an account address and its balance
 *
 * @typedef {Object} AccountBalancePair
 * @property {PublicKey} address
 * @property {number} lamports
 */
type AccountBalancePair = {
  address: PublicKey,
  lamports: number,
};

/**
 * Expected JSON RPC response for the "getLargestAccounts" message
 */
const GetLargestAccountsRpcResult = jsonRpcResultAndContext(
  array(
    pick({
      lamports: number(),
      address: PublicKeyFromString,
    }),
  ),
);

/**
 * @private
 */
const AccountInfoResult = pick({
  executable: boolean(),
  owner: PublicKeyFromString,
  lamports: number(),
  data: BufferFromRawAccountData,
  rentEpoch: number(),
});

/**
 * @private
 */
const KeyedAccountInfoResult = pick({
  pubkey: PublicKeyFromString,
  account: AccountInfoResult,
});

const ParsedOrRawAccountData = coerce(
  union([instance(Buffer), ParsedAccountDataResult]),
  union([RawAccountDataResult, ParsedAccountDataResult]),
  value => {
    if (Array.isArray(value)) {
      return create(value, BufferFromRawAccountData);
    } else {
      return value;
    }
  },
);

/**
 * @private
 */
const ParsedAccountInfoResult = pick({
  executable: boolean(),
  owner: PublicKeyFromString,
  lamports: number(),
  data: ParsedOrRawAccountData,
  rentEpoch: number(),
});

const KeyedParsedAccountInfoResult = pick({
  pubkey: PublicKeyFromString,
  account: ParsedAccountInfoResult,
});

/**
 * @private
 */
const StakeActivationResult = pick({
  state: union([
    literal('active'),
    literal('inactive'),
    literal('activating'),
    literal('deactivating'),
  ]),
  active: number(),
  inactive: number(),
});

/**
 * Expected JSON RPC response for the "getConfirmedSignaturesForAddress" message
 */
const GetConfirmedSignaturesForAddressRpcResult = jsonRpcResult(
  array(string()),
);

/**
 * Expected JSON RPC response for the "getConfirmedSignaturesForAddress2" message
 */

const GetConfirmedSignaturesForAddress2RpcResult = jsonRpcResult(
  array(
    pick({
      signature: string(),
      slot: number(),
      err: TransactionErrorResult,
      memo: nullable(string()),
      blockTime: optional(nullable(number())),
    }),
  ),
);

/***
 * Expected JSON RPC response for the "accountNotification" message
 */
const AccountNotificationResult = pick({
  subscription: number(),
  result: notificationResultAndContext(AccountInfoResult),
});

/**
 * @private
 */
const ProgramAccountInfoResult = pick({
  pubkey: PublicKeyFromString,
  account: AccountInfoResult,
});

/***
 * Expected JSON RPC response for the "programNotification" message
 */
const ProgramAccountNotificationResult = pick({
  subscription: number(),
  result: notificationResultAndContext(ProgramAccountInfoResult),
});

/**
 * @private
 */
const SlotInfoResult = pick({
  parent: number(),
  slot: number(),
  root: number(),
});

/**
 * Expected JSON RPC response for the "slotNotification" message
 */
const SlotNotificationResult = pick({
  subscription: number(),
  result: SlotInfoResult,
});

/**
 * Expected JSON RPC response for the "signatureNotification" message
 */
const SignatureNotificationResult = pick({
  subscription: number(),
  result: notificationResultAndContext(SignatureStatusResult),
});

/**
 * Expected JSON RPC response for the "rootNotification" message
 */
const RootNotificationResult = pick({
  subscription: number(),
  result: number(),
});

const ContactInfoResult = pick({
  pubkey: string(),
  gossip: nullable(string()),
  tpu: nullable(string()),
  rpc: nullable(string()),
  version: nullable(string()),
});

const VoteAccountInfoResult = pick({
  votePubkey: string(),
  nodePubkey: string(),
  activatedStake: number(),
  epochVoteAccount: boolean(),
  epochCredits: array(tuple([number(), number(), number()])),
  commission: number(),
  lastVote: number(),
  rootSlot: nullable(number()),
});

/**
 * Expected JSON RPC response for the "getVoteAccounts" message
 */
const GetVoteAccounts = jsonRpcResult(
  pick({
    current: array(VoteAccountInfoResult),
    delinquent: array(VoteAccountInfoResult),
  }),
);

const SignatureStatusResponse = pick({
  slot: number(),
  confirmations: nullable(number()),
  err: TransactionErrorResult,
  confirmationStatus: optional(nullable(string())),
});

/**
 * Expected JSON RPC response for the "getSignatureStatuses" message
 */
const GetSignatureStatusesRpcResult = jsonRpcResultAndContext(
  array(nullable(SignatureStatusResponse)),
);

/**
 * Expected JSON RPC response for the "getMinimumBalanceForRentExemption" message
 */
const GetMinimumBalanceForRentExemptionRpcResult = jsonRpcResult(number());

/**
 * @private
 */
const ConfirmedTransactionResult = pick({
  signatures: array(string()),
  message: pick({
    accountKeys: array(string()),
    header: pick({
      numRequiredSignatures: number(),
      numReadonlySignedAccounts: number(),
      numReadonlyUnsignedAccounts: number(),
    }),
    instructions: array(
      pick({
        accounts: array(number()),
        data: string(),
        programIdIndex: number(),
      }),
    ),
    recentBlockhash: string(),
  }),
});

const TransactionFromConfirmed = coerce(
  instance(Transaction),
  ConfirmedTransactionResult,
  result => {
    const {message, signatures} = result;
    return Transaction.populate(new Message(message), signatures);
  },
);

const ParsedInstructionResult = pick({
  parsed: unknown(),
  program: string(),
  programId: PublicKeyFromString,
});

const RawInstructionResult = pick({
  accounts: array(PublicKeyFromString),
  data: string(),
  programId: PublicKeyFromString,
});

const InstructionResult = union([
  RawInstructionResult,
  ParsedInstructionResult,
]);

const UnknownInstructionResult = union([
  pick({
    parsed: unknown(),
    program: string(),
    programId: string(),
  }),
  pick({
    accounts: array(string()),
    data: string(),
    programId: string(),
  }),
]);

const ParsedOrRawInstruction = coerce(
  InstructionResult,
  UnknownInstructionResult,
  value => {
    if ('accounts' in value) {
      return create(value, RawInstructionResult);
    } else {
      return create(value, ParsedInstructionResult);
    }
  },
);

/**
 * @private
 */
const ParsedConfirmedTransactionResult = pick({
  signatures: array(string()),
  message: pick({
    accountKeys: array(
      pick({
        pubkey: PublicKeyFromString,
        signer: boolean(),
        writable: boolean(),
      }),
    ),
    instructions: array(ParsedOrRawInstruction),
    recentBlockhash: string(),
  }),
});

const TokenBalanceResult = pick({
  accountIndex: number(),
  mint: string(),
  uiTokenAmount: TokenAmountResult,
});

/**
 * @private
 */
const ConfirmedTransactionMetaResult = pick({
  err: TransactionErrorResult,
  fee: number(),
  innerInstructions: optional(
    nullable(
      array(
        pick({
          index: number(),
          instructions: array(
            pick({
              accounts: array(number()),
              data: string(),
              programIdIndex: number(),
            }),
          ),
        }),
      ),
    ),
  ),
  preBalances: array(number()),
  postBalances: array(number()),
  logMessages: optional(nullable(array(string()))),
  preTokenBalances: optional(nullable(array(TokenBalanceResult))),
  postTokenBalances: optional(nullable(array(TokenBalanceResult))),
});

/**
 * @private
 */
const ParsedConfirmedTransactionMetaResult = pick({
  err: TransactionErrorResult,
  fee: number(),
  innerInstructions: optional(
    nullable(
      array(
        pick({
          index: number(),
          instructions: array(ParsedOrRawInstruction),
        }),
      ),
    ),
  ),
  preBalances: array(number()),
  postBalances: array(number()),
  logMessages: optional(nullable(array(string()))),
  preTokenBalances: optional(nullable(array(TokenBalanceResult))),
  postTokenBalances: optional(nullable(array(TokenBalanceResult))),
});

/**
 * Expected JSON RPC response for the "getConfirmedBlock" message
 */
export const GetConfirmedBlockRpcResult = jsonRpcResult(
  nullable(
    pick({
      blockhash: string(),
      previousBlockhash: string(),
      parentSlot: number(),
      transactions: array(
        pick({
          transaction: TransactionFromConfirmed,
          meta: nullable(ConfirmedTransactionMetaResult),
        }),
      ),
      rewards: optional(
        array(
          pick({
            pubkey: string(),
            lamports: number(),
            postBalance: nullable(number()),
            rewardType: nullable(string()),
          }),
        ),
      ),
    }),
  ),
);

/**
 * Expected JSON RPC response for the "getConfirmedTransaction" message
 */
const GetConfirmedTransactionRpcResult = jsonRpcResult(
  nullable(
    pick({
      slot: number(),
      transaction: TransactionFromConfirmed,
      meta: ConfirmedTransactionMetaResult,
      blockTime: optional(nullable(number())),
    }),
  ),
);

/**
 * Expected JSON RPC response for the "getConfirmedTransaction" message
 */
const GetParsedConfirmedTransactionRpcResult = jsonRpcResult(
  nullable(
    pick({
      slot: number(),
      transaction: ParsedConfirmedTransactionResult,
      meta: nullable(ParsedConfirmedTransactionMetaResult),
      blockTime: optional(nullable(number())),
    }),
  ),
);

/**
 * Expected JSON RPC response for the "getRecentBlockhash" message
 */
const GetRecentBlockhashAndContextRpcResult = jsonRpcResultAndContext(
  pick({
    blockhash: string(),
    feeCalculator: pick({
      lamportsPerSignature: number(),
    }),
  }),
);

const PerfSampleResult = pick({
  slot: number(),
  numTransactions: number(),
  numSlots: number(),
  samplePeriodSecs: number(),
});

/*
 * Expected JSON RPC response for "getRecentPerformanceSamples" message
 */
const GetRecentPerformanceSamplesRpcResult = jsonRpcResult(
  array(PerfSampleResult),
);

/**
 * Expected JSON RPC response for the "getFeeCalculatorForBlockhash" message
 */
const GetFeeCalculatorRpcResult = jsonRpcResultAndContext(
  nullable(
    pick({
      feeCalculator: pick({
        lamportsPerSignature: number(),
      }),
    }),
  ),
);

/**
 * Expected JSON RPC response for the "requestAirdrop" message
 */
const RequestAirdropRpcResult = jsonRpcResult(string());

/**
 * Expected JSON RPC response for the "sendTransaction" message
 */
const SendTransactionRpcResult = jsonRpcResult(string());

/**
 * Information about the latest slot being processed by a node
 *
 * @typedef {Object} SlotInfo
 * @property {number} slot Currently processing slot
 * @property {number} parent Parent of the current slot
 * @property {number} root The root block of the current slot's fork
 */
export type SlotInfo = {
  slot: number,
  parent: number,
  root: number,
};

/**
 * Parsed account data
 *
 * @typedef {Object} ParsedAccountData
 * @property {string} program Name of the program that owns this account
 * @property {any} parsed Parsed account data
 * @property {number} space Space used by account data
 */
type ParsedAccountData = {
  program: string,
  parsed: any,
  space: number,
};

/**
 * Stake Activation data
 *
 * @typedef {Object} StakeActivationData
 * @property {string} state: <string - the stake account's activation state, one of: active, inactive, activating, deactivating
 * @property {number} active: stake active during the epoch
 * @property {number} inactive: stake inactive during the epoch
 */
type StakeActivationData = {
  state: 'active' | 'inactive' | 'activating' | 'deactivating',
  active: number,
  inactive: number,
};

/**
 * Information describing an account
 *
 * @typedef {Object} AccountInfo
 * @property {number} lamports Number of lamports assigned to the account
 * @property {PublicKey} owner Identifier of the program that owns the account
 * @property {T} data Optional data assigned to the account
 * @property {boolean} executable `true` if this account's data contains a loaded program
 */
type AccountInfo<T> = {
  executable: boolean,
  owner: PublicKey,
  lamports: number,
  data: T,
};

/**
 * Account information identified by pubkey
 *
 * @typedef {Object} KeyedAccountInfo
 * @property {PublicKey} accountId
 * @property {AccountInfo<Buffer>} accountInfo
 */
export type KeyedAccountInfo = {
  accountId: PublicKey,
  accountInfo: AccountInfo<Buffer>,
};

/**
 * Callback function for account change notifications
 */
export type AccountChangeCallback = (
  accountInfo: AccountInfo<Buffer>,
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
  commitment: ?Commitment,
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
  commitment: ?Commitment,
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
  commitment: ?Commitment,
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
 * @property {string | null} confirmationStatus the transaction's cluster confirmation status, if data available. Possible non-null responses: `processed`, `confirmed`, `finalized`
 */
export type SignatureStatus = {
  slot: number,
  confirmations: number | null,
  err: TransactionError | null,
  confirmationStatus: string | null,
};

/**
 * A confirmed signature with its status
 *
 * @typedef {Object} ConfirmedSignatureInfo
 * @property {string} signature the transaction signature
 * @property {number} slot when the transaction was processed
 * @property {TransactionError | null} err error, if any
 * @property {string | null} memo memo associated with the transaction, if any
 * @property {number | null | undefined} blockTime The unix timestamp of when the transaction was processed
 */
export type ConfirmedSignatureInfo = {
  signature: string,
  slot: number,
  err: TransactionError | null,
  memo: string | null,
  blockTime?: number | null,
};

/**
 * A connection to a fullnode JSON RPC endpoint
 */
export class Connection {
  _rpcEndpoint: string;
  _rpcRequest: RpcRequest;
  _rpcWebSocket: RpcWebSocketClient;
  _rpcWebSocketConnected: boolean = false;
  _rpcWebSocketHeartbeat: IntervalID | null = null;
  _rpcWebSocketIdleTimeout: TimeoutID | null = null;

  _commitment: ?Commitment;
  _blockhashInfo: {
    recentBlockhash: Blockhash | null,
    lastFetch: Date,
    simulatedSignatures: Array<string>,
    transactionSignatures: Array<string>,
  };
  _disableBlockhashCaching: boolean = false;
  _pollingBlockhash: boolean = false;
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
    this._rpcEndpoint = endpoint;

    let url = urlParse(endpoint);
    const useHttps = url.protocol === 'https:';

    this._rpcRequest = createRpcRequest(url.href, useHttps);
    this._commitment = commitment;
    this._blockhashInfo = {
      recentBlockhash: null,
      lastFetch: new Date(0),
      transactionSignatures: [],
      simulatedSignatures: [],
    };

    url.protocol = useHttps ? 'wss:' : 'ws:';
    url.host = '';
    // Only shift the port by +1 as a convention for ws(s) only if given endpoint
    // is explictly specifying the endpoint port (HTTP-based RPC), assuming
    // we're directly trying to connect to solana-validator's ws listening port.
    // When the endpoint omits the port, we're connecting to the protocol
    // default ports: http(80) or https(443) and it's assumed we're behind a reverse
    // proxy which manages WebSocket upgrade and backend port redirection.
    if (url.port !== null) {
      url.port = String(Number(url.port) + 1);
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
    const args = this._buildArgs([publicKey.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getBalance', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(number()));
    if (res.error) {
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
   * Fetch the estimated production time of a block
   */
  async getBlockTime(slot: number): Promise<number | null> {
    const unsafeRes = await this._rpcRequest('getBlockTime', [slot]);
    const res = create(unsafeRes, jsonRpcResult(nullable(number())));
    if (res.error) {
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
    if (res.error) {
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
    const res = create(unsafeRes, SlotRpcResult);
    if (res.error) {
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
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<Supply>> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSupply', args);
    const res = create(unsafeRes, GetSupplyRpcResult);
    if (res.error) {
      throw new Error('failed to get supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current supply of a token mint
   */
  async getTokenSupply(
    tokenMintAddress: PublicKey,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<TokenAmount>> {
    const args = this._buildArgs([tokenMintAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenSupply', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(TokenAmountResult));
    if (res.error) {
      throw new Error('failed to get token supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current balance of a token account
   */
  async getTokenAccountBalance(
    tokenAddress: PublicKey,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<TokenAmount>> {
    const args = this._buildArgs([tokenAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenAccountBalance', args);
    const res = create(unsafeRes, jsonRpcResultAndContext(TokenAmountResult));
    if (res.error) {
      throw new Error(
        'failed to get token account balance: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the token accounts owned by the specified account
   *
   * @return {Promise<RpcResponseAndContext<Array<{pubkey: PublicKey, account: AccountInfo<Buffer>}>>>}
   */
  async getTokenAccountsByOwner(
    ownerAddress: PublicKey,
    filter: TokenAccountsFilter,
    commitment: ?Commitment,
  ): Promise<
    RpcResponseAndContext<
      Array<{pubkey: PublicKey, account: AccountInfo<Buffer>}>,
    >,
  > {
    let _args = [ownerAddress.toBase58()];
    if (filter.mint) {
      _args.push({mint: filter.mint.toBase58()});
    } else {
      _args.push({programId: filter.programId.toBase58()});
    }

    const args = this._buildArgs(_args, commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getTokenAccountsByOwner', args);
    const res = create(unsafeRes, GetTokenAccountsByOwner);
    if (res.error) {
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
    commitment: ?Commitment,
  ): Promise<
    RpcResponseAndContext<
      Array<{pubkey: PublicKey, account: AccountInfo<ParsedAccountData>}>,
    >,
  > {
    let _args = [ownerAddress.toBase58()];
    if (filter.mint) {
      _args.push({mint: filter.mint.toBase58()});
    } else {
      _args.push({programId: filter.programId.toBase58()});
    }

    const args = this._buildArgs(_args, commitment, 'jsonParsed');
    const unsafeRes = await this._rpcRequest('getTokenAccountsByOwner', args);
    const res = create(unsafeRes, GetParsedTokenAccountsByOwner);
    if (res.error) {
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
    config: ?GetLargestAccountsConfig,
  ): Promise<RpcResponseAndContext<Array<AccountBalancePair>>> {
    const arg = {
      ...config,
      commitment: (config && config.commitment) || this.commitment,
    };
    const args = arg.filter || arg.commitment ? [arg] : [];
    const unsafeRes = await this._rpcRequest('getLargestAccounts', args);
    const res = create(unsafeRes, GetLargestAccountsRpcResult);
    if (res.error) {
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
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<Array<TokenAccountBalancePair>>> {
    const args = this._buildArgs([mintAddress.toBase58()], commitment);
    const unsafeRes = await this._rpcRequest('getTokenLargestAccounts', args);
    const res = create(unsafeRes, GetTokenLargestAccountsResult);
    if (res.error) {
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
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<AccountInfo<Buffer> | null>> {
    const args = this._buildArgs([publicKey.toBase58()], commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getAccountInfo', args);
    const res = create(
      unsafeRes,
      jsonRpcResultAndContext(nullable(AccountInfoResult)),
    );
    if (res.error) {
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
    commitment: ?Commitment,
  ): Promise<
    RpcResponseAndContext<AccountInfo<Buffer | ParsedAccountData> | null>,
  > {
    const args = this._buildArgs(
      [publicKey.toBase58()],
      commitment,
      'jsonParsed',
    );
    const unsafeRes = await this._rpcRequest('getAccountInfo', args);
    const res = create(
      unsafeRes,
      jsonRpcResultAndContext(nullable(ParsedAccountInfoResult)),
    );
    if (res.error) {
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
    commitment: ?Commitment,
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
   * Returns epoch activation information for a stake account that has been delegated
   */
  async getStakeActivation(
    publicKey: PublicKey,
    commitment: ?Commitment,
    epoch: ?number,
  ): Promise<StakeActivationData> {
    const args = this._buildArgs(
      [publicKey.toBase58()],
      commitment,
      undefined,
      epoch !== undefined ? {epoch} : undefined,
    );

    const unsafeRes = await this._rpcRequest('getStakeActivation', args);
    const res = create(unsafeRes, jsonRpcResult(StakeActivationResult));
    if (res.error) {
      throw new Error(
        `failed to get Stake Activation ${publicKey.toBase58()}: ${
          res.error.message
        }`,
      );
    }
    return res.result;
  }

  /**
   * Fetch all the accounts owned by the specified program id
   *
   * @return {Promise<Array<{pubkey: PublicKey, account: AccountInfo<Buffer>}>>}
   */
  async getProgramAccounts(
    programId: PublicKey,
    commitment: ?Commitment,
  ): Promise<Array<{pubkey: PublicKey, account: AccountInfo<Buffer>}>> {
    const args = this._buildArgs([programId.toBase58()], commitment, 'base64');
    const unsafeRes = await this._rpcRequest('getProgramAccounts', args);
    const res = create(unsafeRes, jsonRpcResult(array(KeyedAccountInfoResult)));
    if (res.error) {
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
   *
   * @return {Promise<Array<{pubkey: PublicKey, account: AccountInfo<Buffer | ParsedAccountData>}>>}
   */
  async getParsedProgramAccounts(
    programId: PublicKey,
    commitment: ?Commitment,
  ): Promise<
    Array<{
      pubkey: PublicKey,
      account: AccountInfo<Buffer | ParsedAccountData>,
    }>,
  > {
    const args = this._buildArgs(
      [programId.toBase58()],
      commitment,
      'jsonParsed',
    );
    const unsafeRes = await this._rpcRequest('getProgramAccounts', args);
    const res = create(
      unsafeRes,
      jsonRpcResult(array(KeyedParsedAccountInfoResult)),
    );
    if (res.error) {
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
   * Confirm the transaction identified by the specified signature.
   */
  async confirmTransaction(
    signature: TransactionSignature,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<SignatureResult>> {
    let decodedSignature;
    try {
      decodedSignature = bs58.decode(signature);
    } catch (err) {
      throw new Error('signature must be base58 encoded: ' + signature);
    }

    assert(decodedSignature.length === 64, 'signature has invalid length');

    const start = Date.now();
    const subscriptionCommitment = commitment || this.commitment;

    let subscriptionId;
    let response: RpcResponseAndContext<SignatureResult> | null = null;
    const confirmPromise = new Promise((resolve, reject) => {
      try {
        subscriptionId = this.onSignature(
          signature,
          (result, context) => {
            subscriptionId = undefined;
            response = {
              context,
              value: result,
            };
            resolve();
          },
          subscriptionCommitment,
        );
      } catch (err) {
        reject(err);
      }
    });

    let timeoutMs = 60 * 1000;
    switch (subscriptionCommitment) {
      case 'processed':
      case 'recent':
      case 'single':
      case 'confirmed':
      case 'singleGossip': {
        timeoutMs = 30 * 1000;
        break;
      }
      // exhaust enums to ensure full coverage
      case 'finalized':
      case 'max':
      case 'root':
    }

    try {
      await promiseTimeout(confirmPromise, timeoutMs);
    } finally {
      if (subscriptionId) {
        this.removeSignatureListener(subscriptionId);
      }
    }

    if (response === null) {
      const duration = (Date.now() - start) / 1000;
      throw new Error(
        `Transaction was not confirmed in ${duration.toFixed(
          2,
        )} seconds. It is unknown if it succeeded or failed. Check signature ${signature} using the Solana Explorer or CLI tools.`,
      );
    }

    return response;
  }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getClusterNodes(): Promise<Array<ContactInfo>> {
    const unsafeRes = await this._rpcRequest('getClusterNodes', []);
    const res = create(unsafeRes, jsonRpcResult(array(ContactInfoResult)));
    if (res.error) {
      throw new Error('failed to get cluster nodes: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Return the list of nodes that are currently participating in the cluster
   */
  async getVoteAccounts(commitment: ?Commitment): Promise<VoteAccountStatus> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getVoteAccounts', args);
    const res = create(unsafeRes, GetVoteAccounts);
    if (res.error) {
      throw new Error('failed to get vote accounts: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current slot that the node is processing
   */
  async getSlot(commitment: ?Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSlot', args);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if (res.error) {
      throw new Error('failed to get slot: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current slot leader of the cluster
   */
  async getSlotLeader(commitment: ?Commitment): Promise<string> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getSlotLeader', args);
    const res = create(unsafeRes, jsonRpcResult(string()));
    if (res.error) {
      throw new Error('failed to get slot leader: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current status of a signature
   */
  async getSignatureStatus(
    signature: TransactionSignature,
    config: ?SignatureStatusConfig,
  ): Promise<RpcResponseAndContext<SignatureStatus | null>> {
    const {context, value: values} = await this.getSignatureStatuses(
      [signature],
      config,
    );
    assert(values.length === 1);
    const value = values[0];
    return {context, value};
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
    const res = create(unsafeRes, GetSignatureStatusesRpcResult);
    if (res.error) {
      throw new Error('failed to get signature status: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current transaction count of the cluster
   */
  async getTransactionCount(commitment: ?Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getTransactionCount', args);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if (res.error) {
      throw new Error('failed to get transaction count: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the current total currency supply of the cluster in lamports
   */
  async getTotalSupply(commitment: ?Commitment): Promise<number> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getTotalSupply', args);
    const res = create(unsafeRes, jsonRpcResult(number()));
    if (res.error) {
      throw new Error('failed to get total supply: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the cluster InflationGovernor parameters
   */
  async getInflationGovernor(
    commitment: ?Commitment,
  ): Promise<InflationGovernor> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getInflationGovernor', args);
    const res = create(unsafeRes, GetInflationGovernorRpcResult);
    if (res.error) {
      throw new Error('failed to get inflation: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the Epoch Info parameters
   */
  async getEpochInfo(commitment: ?Commitment): Promise<EpochInfo> {
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getEpochInfo', args);
    const res = create(unsafeRes, GetEpochInfoRpcResult);
    if (res.error) {
      throw new Error('failed to get epoch info: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the Epoch Schedule parameters
   */
  async getEpochSchedule(): Promise<EpochSchedule> {
    const unsafeRes = await this._rpcRequest('getEpochSchedule', []);
    const res = create(unsafeRes, GetEpochScheduleRpcResult);
    if (res.error) {
      throw new Error('failed to get epoch schedule: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the leader schedule for the current epoch
   * @return {Promise<RpcResponseAndContext<LeaderSchedule>>}
   */
  async getLeaderSchedule(): Promise<LeaderSchedule> {
    const unsafeRes = await this._rpcRequest('getLeaderSchedule', []);
    const res = create(unsafeRes, GetLeaderScheduleRpcResult);
    if (res.error) {
      throw new Error('failed to get leader schedule: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch the minimum balance needed to exempt an account of `dataLength`
   * size from rent
   */
  async getMinimumBalanceForRentExemption(
    dataLength: number,
    commitment: ?Commitment,
  ): Promise<number> {
    const args = this._buildArgs([dataLength], commitment);
    const unsafeRes = await this._rpcRequest(
      'getMinimumBalanceForRentExemption',
      args,
    );
    const res = create(unsafeRes, GetMinimumBalanceForRentExemptionRpcResult);
    if (res.error) {
      console.warn('Unable to fetch minimum balance for rent exemption');
      return 0;
    }
    return res.result;
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
    const args = this._buildArgs([], commitment);
    const unsafeRes = await this._rpcRequest('getRecentBlockhash', args);
    const res = create(unsafeRes, GetRecentBlockhashAndContextRpcResult);
    if (res.error) {
      throw new Error('failed to get recent blockhash: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Fetch recent performance samples
   * @return {Promise<Array<PerfSample>>}
   */
  async getRecentPerformanceSamples(
    limit: ?number,
  ): Promise<Array<PerfSample>> {
    const args = this._buildArgs(limit ? [limit] : []);
    const unsafeRes = await this._rpcRequest(
      'getRecentPerformanceSamples',
      args,
    );
    const res = create(unsafeRes, GetRecentPerformanceSamplesRpcResult);
    if (res.error) {
      throw new Error(
        'failed to get recent performance samples: ' + res.error.message,
      );
    }

    return res.result;
  }

  /**
   * Fetch the fee calculator for a recent blockhash from the cluster, return with context
   */
  async getFeeCalculatorForBlockhash(
    blockhash: Blockhash,
    commitment: ?Commitment,
  ): Promise<RpcResponseAndContext<FeeCalculator | null>> {
    const args = this._buildArgs([blockhash], commitment);
    const unsafeRes = await this._rpcRequest(
      'getFeeCalculatorForBlockhash',
      args,
    );

    const res = create(unsafeRes, GetFeeCalculatorRpcResult);
    if (res.error) {
      throw new Error('failed to get fee calculator: ' + res.error.message);
    }
    const {context, value} = res.result;
    return {
      context,
      value: value.feeCalculator,
    };
  }

  /**
   * Fetch a recent blockhash from the cluster
   * @return {Promise<{blockhash: Blockhash, feeCalculator: FeeCalculator}>}
   */
  async getRecentBlockhash(
    commitment: ?Commitment,
  ): Promise<{blockhash: Blockhash, feeCalculator: FeeCalculator}> {
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
    if (res.error) {
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
    const res = create(unsafeRes, GetConfirmedBlockRpcResult);
    if (res.error) {
      throw new Error('failed to get confirmed block: ' + res.error.message);
    }
    const result = res.result;
    if (!result) {
      throw new Error('Confirmed block ' + slot + ' not found');
    }
    return result;
  }

  /**
   * Fetch a transaction details for a confirmed transaction
   */
  async getConfirmedTransaction(
    signature: TransactionSignature,
  ): Promise<ConfirmedTransaction | null> {
    const unsafeRes = await this._rpcRequest('getConfirmedTransaction', [
      signature,
    ]);
    const res = create(unsafeRes, GetConfirmedTransactionRpcResult);
    if (res.error) {
      throw new Error(
        'failed to get confirmed transaction: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch parsed transaction details for a confirmed transaction
   */
  async getParsedConfirmedTransaction(
    signature: TransactionSignature,
  ): Promise<ParsedConfirmedTransaction | null> {
    const unsafeRes = await this._rpcRequest('getConfirmedTransaction', [
      signature,
      'jsonParsed',
    ]);
    const res = create(unsafeRes, GetParsedConfirmedTransactionRpcResult);
    if (res.error) {
      throw new Error(
        'failed to get confirmed transaction: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Fetch a list of all the confirmed signatures for transactions involving an address
   * within a specified slot range. Max range allowed is 10,000 slots.
   *
   * @param address queried address
   * @param startSlot start slot, inclusive
   * @param endSlot end slot, inclusive
   */
  async getConfirmedSignaturesForAddress(
    address: PublicKey,
    startSlot: number,
    endSlot: number,
  ): Promise<Array<TransactionSignature>> {
    const unsafeRes = await this._rpcRequest(
      'getConfirmedSignaturesForAddress',
      [address.toBase58(), startSlot, endSlot],
    );
    const res = create(unsafeRes, GetConfirmedSignaturesForAddressRpcResult);
    if (res.error) {
      throw new Error(
        'failed to get confirmed signatures for address: ' + res.error.message,
      );
    }
    return res.result;
  }

  /**
   * Returns confirmed signatures for transactions involving an
   * address backwards in time from the provided signature or most recent confirmed block
   *
   *
   * @param address queried address
   * @param options
   */
  async getConfirmedSignaturesForAddress2(
    address: PublicKey,
    options: ?ConfirmedSignaturesForAddress2Options,
  ): Promise<Array<ConfirmedSignatureInfo>> {
    const unsafeRes = await this._rpcRequest(
      'getConfirmedSignaturesForAddress2',
      [address.toBase58(), options],
    );
    const res = create(unsafeRes, GetConfirmedSignaturesForAddress2RpcResult);
    if (res.error) {
      throw new Error(
        'failed to get confirmed signatures for address: ' + res.error.message,
      );
    }
    return res.result;
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
  ): Promise<TransactionSignature> {
    const unsafeRes = await this._rpcRequest('requestAirdrop', [
      to.toBase58(),
      amount,
    ]);
    const res = create(unsafeRes, RequestAirdropRpcResult);
    if (res.error) {
      throw new Error(
        'airdrop to ' + to.toBase58() + ' failed: ' + res.error.message,
      );
    }
    return res.result;
  }

  async _recentBlockhash(disableCache: boolean): Promise<Blockhash> {
    if (!disableCache) {
      // Wait for polling to finish
      while (this._pollingBlockhash) {
        await sleep(100);
      }
      // Attempt to use a recent blockhash for up to 30 seconds
      const expired =
        Date.now() - this._blockhashInfo.lastFetch >=
        BLOCKHASH_CACHE_TIMEOUT_MS;
      if (this._blockhashInfo.recentBlockhash !== null && !expired) {
        return this._blockhashInfo.recentBlockhash;
      }
    }

    return await this._pollNewBlockhash();
  }

  async _pollNewBlockhash(): Promise<Blockhash> {
    this._pollingBlockhash = true;
    try {
      const startTime = Date.now();
      for (let i = 0; i < 50; i++) {
        const {blockhash} = await this.getRecentBlockhash('finalized');

        if (this._blockhashInfo.recentBlockhash != blockhash) {
          this._blockhashInfo = {
            recentBlockhash: blockhash,
            lastFetch: new Date(),
            transactionSignatures: [],
            simulatedSignatures: [],
          };
          return blockhash;
        }

        // Sleep for approximately half a slot
        await sleep(MS_PER_SLOT / 2);
      }

      throw new Error(
        `Unable to obtain a new blockhash after ${Date.now() - startTime}ms`,
      );
    } finally {
      this._pollingBlockhash = false;
    }
  }

  /**
   * Simulate a transaction
   */
  async simulateTransaction(
    transaction: Transaction,
    signers?: Array<Account>,
  ): Promise<RpcResponseAndContext<SimulatedTransactionResponse>> {
    if (transaction.nonceInfo && signers) {
      transaction.sign(...signers);
    } else {
      let disableCache = this._disableBlockhashCaching;
      for (;;) {
        transaction.recentBlockhash = await this._recentBlockhash(disableCache);

        if (!signers) break;

        transaction.sign(...signers);
        if (!transaction.signature) {
          throw new Error('!signature'); // should never happen
        }

        // If the signature of this transaction has not been seen before with the
        // current recentBlockhash, all done.
        const signature = transaction.signature.toString('base64');
        if (
          !this._blockhashInfo.simulatedSignatures.includes(signature) &&
          !this._blockhashInfo.transactionSignatures.includes(signature)
        ) {
          this._blockhashInfo.simulatedSignatures.push(signature);
          break;
        } else {
          disableCache = true;
        }
      }
    }

    const signData = transaction.serializeMessage();
    const wireTransaction = transaction._serialize(signData);
    const encodedTransaction = wireTransaction.toString('base64');
    const config: any = {
      encoding: 'base64',
      commitment: this.commitment,
    };
    const args = [encodedTransaction, config];

    if (signers) {
      config.sigVerify = true;
    }

    const unsafeRes = await this._rpcRequest('simulateTransaction', args);
    const res = create(unsafeRes, SimulatedTransactionResponseStruct);
    if (res.error) {
      throw new Error('failed to simulate transaction: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * Sign and send a transaction
   */
  async sendTransaction(
    transaction: Transaction,
    signers: Array<Account>,
    options?: SendOptions,
  ): Promise<TransactionSignature> {
    if (transaction.nonceInfo) {
      transaction.sign(...signers);
    } else {
      let disableCache = this._disableBlockhashCaching;
      for (;;) {
        transaction.recentBlockhash = await this._recentBlockhash(disableCache);
        transaction.sign(...signers);
        if (!transaction.signature) {
          throw new Error('!signature'); // should never happen
        }

        // If the signature of this transaction has not been seen before with the
        // current recentBlockhash, all done.
        const signature = transaction.signature.toString('base64');
        if (!this._blockhashInfo.transactionSignatures.includes(signature)) {
          this._blockhashInfo.transactionSignatures.push(signature);
          break;
        } else {
          disableCache = true;
        }
      }
    }

    const wireTransaction = transaction.serialize();
    return await this.sendRawTransaction(wireTransaction, options);
  }

  /**
   * Send a transaction that has already been signed and serialized into the
   * wire format
   */
  async sendRawTransaction(
    rawTransaction: Buffer | Uint8Array | Array<number>,
    options: ?SendOptions,
  ): Promise<TransactionSignature> {
    const encodedTransaction = toBuffer(rawTransaction).toString('base64');
    const result = await this.sendEncodedTransaction(
      encodedTransaction,
      options,
    );
    return result;
  }

  /**
   * Send a transaction that has already been signed, serialized into the
   * wire format, and encoded as a base64 string
   */
  async sendEncodedTransaction(
    encodedTransaction: string,
    options: ?SendOptions,
  ): Promise<TransactionSignature> {
    const config: any = {encoding: 'base64'};
    const args = [encodedTransaction, config];
    const skipPreflight = options && options.skipPreflight;
    const preflightCommitment =
      (options && options.preflightCommitment) || this.commitment;

    if (skipPreflight) {
      config.skipPreflight = skipPreflight;
    }
    if (preflightCommitment) {
      config.preflightCommitment = preflightCommitment;
    }

    const unsafeRes = await this._rpcRequest('sendTransaction', args);
    const res = create(unsafeRes, SendTransactionRpcResult);
    if (res.error) {
      if (res.error.data) {
        const logs = res.error.data.logs;
        if (logs && Array.isArray(logs)) {
          const traceIndent = '\n    ';
          const logTrace = traceIndent + logs.join(traceIndent);
          console.error(res.error.message, logTrace);
        }
      }
      throw new Error('failed to send transaction: ' + res.error.message);
    }
    return res.result;
  }

  /**
   * @private
   */
  _wsOnOpen() {
    this._rpcWebSocketConnected = true;
    this._rpcWebSocketHeartbeat = setInterval(() => {
      // Ping server every 5s to prevent idle timeouts
      this._rpcWebSocket.notify('ping').catch(() => {});
    }, 5000);
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
  _wsOnClose(code: number) {
    clearInterval(this._rpcWebSocketHeartbeat);
    this._rpcWebSocketHeartbeat = null;

    if (code === 1000) {
      // explicit close, check if any subscriptions have been made since close
      this._updateSubscriptions();
      return;
    }

    // implicit close, prepare subscriptions for auto-reconnect
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
      if (this._rpcWebSocketConnected) {
        this._rpcWebSocketConnected = false;
        this._rpcWebSocketIdleTimeout = setTimeout(() => {
          this._rpcWebSocketIdleTimeout = null;
          this._rpcWebSocket.close();
        }, 500);
      }
      return;
    }

    if (this._rpcWebSocketIdleTimeout !== null) {
      clearTimeout(this._rpcWebSocketIdleTimeout);
      this._rpcWebSocketIdleTimeout = null;
      this._rpcWebSocketConnected = true;
    }

    if (!this._rpcWebSocketConnected) {
      this._rpcWebSocket.connect();
      return;
    }

    for (let id of accountKeys) {
      const sub = this._accountChangeSubscriptions[id];
      this._subscribe(
        sub,
        'accountSubscribe',
        this._buildArgs([sub.publicKey], sub.commitment, 'base64'),
      );
    }

    for (let id of programKeys) {
      const sub = this._programAccountChangeSubscriptions[id];
      this._subscribe(
        sub,
        'programSubscribe',
        this._buildArgs([sub.programId], sub.commitment, 'base64'),
      );
    }

    for (let id of slotKeys) {
      const sub = this._slotSubscriptions[id];
      this._subscribe(sub, 'slotSubscribe', []);
    }

    for (let id of signatureKeys) {
      const sub = this._signatureSubscriptions[id];
      this._subscribe(
        sub,
        'signatureSubscribe',
        this._buildArgs([sub.signature], sub.commitment),
      );
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
    const res = create(notification, AccountNotificationResult);
    if (res.error) {
      throw new Error('account notification failed: ' + res.error.message);
    }
    const keys = Object.keys(this._accountChangeSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._accountChangeSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        sub.callback(res.result.value, res.result.context);
        return true;
      }
    }
  }

  /**
   * Register a callback to be invoked whenever the specified account changes
   *
   * @param publicKey Public key of the account to monitor
   * @param callback Function to invoke whenever the account is changed
   * @param commitment Specify the commitment level account changes must reach before notification
   * @return subscription id
   */
  onAccountChange(
    publicKey: PublicKey,
    callback: AccountChangeCallback,
    commitment: ?Commitment,
  ): number {
    const id = ++this._accountChangeSubscriptionCounter;
    this._accountChangeSubscriptions[id] = {
      publicKey: publicKey.toBase58(),
      callback,
      commitment,
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
    const res = create(notification, ProgramAccountNotificationResult);
    if (res.error) {
      throw new Error(
        'program account notification failed: ' + res.error.message,
      );
    }
    const keys = Object.keys(this._programAccountChangeSubscriptions).map(
      Number,
    );
    for (let id of keys) {
      const sub = this._programAccountChangeSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        const {value, context} = res.result;
        sub.callback(
          {
            accountId: value.pubkey,
            accountInfo: value.account,
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
   * @param commitment Specify the commitment level account changes must reach before notification
   * @return subscription id
   */
  onProgramAccountChange(
    programId: PublicKey,
    callback: ProgramAccountChangeCallback,
    commitment: ?Commitment,
  ): number {
    const id = ++this._programAccountChangeSubscriptionCounter;
    this._programAccountChangeSubscriptions[id] = {
      programId: programId.toBase58(),
      callback,
      commitment,
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
    const res = create(notification, SlotNotificationResult);
    if (res.error) {
      throw new Error('slot notification failed: ' + res.error.message);
    }
    const keys = Object.keys(this._slotSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._slotSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        sub.callback(res.result);
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

  _buildArgs(
    args: Array<any>,
    override: ?Commitment,
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

  /**
   * @private
   */
  _wsOnSignatureNotification(notification: Object) {
    const res = create(notification, SignatureNotificationResult);
    if (res.error) {
      throw new Error('signature notification failed: ' + res.error.message);
    }
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
   * @param commitment Specify the commitment level signature must reach before notification
   * @return subscription id
   */
  onSignature(
    signature: TransactionSignature,
    callback: SignatureResultCallback,
    commitment: ?Commitment,
  ): number {
    const id = ++this._signatureSubscriptionCounter;
    this._signatureSubscriptions[id] = {
      signature,
      callback,
      commitment,
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
    const res = create(notification, RootNotificationResult);
    if (res.error) {
      throw new Error('root notification failed: ' + res.error.message);
    }
    const keys = Object.keys(this._rootSubscriptions).map(Number);
    for (let id of keys) {
      const sub = this._rootSubscriptions[id];
      if (sub.subscriptionId === res.subscription) {
        sub.callback(res.result);
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
