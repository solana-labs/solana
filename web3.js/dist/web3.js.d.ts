/// <reference types="node" />
import BN from 'bn.js';

/**
 * Blockhash as Base58 string.
 * @public
 */
declare type Blockhash = string;

/**
 * The expiration time of a recently cached blockhash in milliseconds.
 * @internal
 */
export declare const BLOCKHASH_CACHE_TIMEOUT_MS: number;

/**
 * The level of commitment desired when querying state.
 * ```
 * 'max':          Query the most recent block which has been finalized by the cluster
 * 'recent':       Query the most recent block which has reached 1 confirmation by the connected node
 * 'root':         Query the most recent block which has been rooted by the connected node
 * 'single':       Query the most recent block which has reached 1 confirmation by the cluster
 * 'singleGossip': Query the most recent block which has reached 1 confirmation according to votes seen in gossip
 * ```
 * @public
 */
export declare type Commitment = 'max' | 'recent' | 'root' | 'single' | 'singleGossip';

/**
 * A connection to a fullnode JSON RPC endpoint.
 * @public
 */
export declare class Connection {
    private _rpcEndpoint;
    private _rpcRequest;
    private _commitment;
    /**
     * Establish a JSON RPC connection
     *
     * @param endpoint - URL to the fullnode JSON RPC endpoint
     * @param commitment - optional default commitment level
     */
    constructor(endpoint: string, commitment?: Commitment);
    /**
     * Fetch the current statuses of a batch of signatures
     */
    getSignatureStatuses(signatures: Array<TransactionSignature>, config?: SignatureStatusConfig): Promise<RpcResponseAndContext<Array<SignatureStatus | null>>>;
    /**
     * Request an allocation of lamports to the specified account
     */
    requestAirdrop(to: PublicKey, amount: number): Promise<TransactionSignature>;
    private _buildArgs;
}

/**
 * Information about the current epoch.
 * @public
 */
export declare interface EpochInfo {
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

/**
 * Epoch schedule
 * (see {@link https://docs.solana.com/terminology#epoch | terminology})
 * @public
 */
export declare interface EpochSchedule {
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

/**
 * Calculator for transaction fees.
 * @public
 */
declare interface FeeCalculator {
    /**
     * Cost in lamports to validate a signature.
     */
    lamportsPerSignature: number;
}

/**
 * Network Inflation
 * (see {@link https://docs.solana.com/inflation/inflation_schedule | proposed schedule})
 * @public
 */
export declare interface InflationGovernor {
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

/**
 * A performance sample
 * @public
 */
export declare interface PerfSample {
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

/**
 * A public key
 */
declare class PublicKey {
    _bn: BN;
    /**
     * Create a new PublicKey object
     */
    constructor(value: number | string | Buffer | Uint8Array | Array<number>);
    /**
     * Checks if two publicKeys are equal
     */
    equals(publicKey: PublicKey): boolean;
    /**
     * Return the base-58 representation of the public key
     */
    toBase58(): string;
    /**
     * Return the Buffer representation of the public key
     */
    toBuffer(): Buffer;
    /**
     * Returns a string representation of the public key
     */
    toString(): string;
    /**
     * Derive a public key from another key, a seed, and a program ID.
     */
    static createWithSeed(fromPublicKey: PublicKey, seed: string, programId: PublicKey): Promise<PublicKey>;
    /**
     * Derive a program address from seeds and a program ID.
     */
    static createProgramAddress(seeds: Array<Buffer | Uint8Array>, programId: PublicKey): Promise<PublicKey>;
    /**
     * Find a valid program address
     *
     * Valid program addresses must fall off the ed25519 curve.  This function
     * iterates a nonce until it finds one that when combined with the seeds
     * results in a valid program address.
     */
    static findProgramAddress(seeds: Array<Buffer | Uint8Array>, programId: PublicKey): Promise<PublicKeyNonce>;
}

declare type PublicKeyNonce = [PublicKey, number];

/**
 * Recent string blockhash and fee calculator
 * @public
 */
export declare interface RecentBlockhash {
    /**
     * A blockhash as base-58 encoded string.
     */
    blockhash: Blockhash;
    /**
     * The fee calculator for this blockhash.
     */
    feeCalculator: FeeCalculator;
}

/**
 * RPC response which includes the slot at which the operation was evaluated.
 * @public
 */
export declare type RpcResponseAndContext<T> = {
    context: {
        slot: number;
    };
    value: T;
};

/**
 * Transaction signature status.
 * @public
 */
export declare interface SignatureStatus {
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
}

/**
 * Configuration object for changing signature status query behavior.
 * @public
 */
export declare interface SignatureStatusConfig {
    /**
     * Enable searching status history, not needed for recent transactions.
     */
    searchTransactionHistory: boolean;
}

/**
 * Transaction error object.
 * @public
 */
export declare type TransactionError = object;

/**
 * Transaction signature as Base58 string.
 * @public
 */
export declare type TransactionSignature = string;

/**
 * Version info for a node.
 * @public
 */
export declare interface Version {
    /**
     * The current version of solana-core.
     */
    'solana-core': string;
    /**
     * The first 4 bytes of the FeatureSet identifier
     */
    'feature-set': number | null;
}

export { }
