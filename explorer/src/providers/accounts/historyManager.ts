import {
  TransactionSignature,
  Connection,
  PublicKey,
  SignatureStatus,
} from "@solana/web3.js";

const MAX_STATUS_BATCH_SIZE = 256;

export interface SlotRange {
  min: number;
  max: number;
}

export type HistoricalTransaction = {
  signature: TransactionSignature;
  status: SignatureStatus;
};

// Manage the transaction history for accounts for a cluster
export class HistoryManager {
  accountRanges: Map<string, SlotRange> = new Map();
  accountHistory: Map<string, HistoricalTransaction[]> = new Map();
  accountLock: Map<string, boolean> = new Map();
  _fullRange: SlotRange | undefined;
  connection: Connection;

  constructor(url: string) {
    this.connection = new Connection(url);
  }

  async fullRange(refresh: boolean): Promise<SlotRange> {
    if (refresh || !this._fullRange) {
      const max = (await this.connection.getEpochInfo("max")).absoluteSlot;
      this._fullRange = { min: 0, max };
    }
    return this._fullRange;
  }

  removeAccountHistory(address: string) {
    this.accountLock.delete(address);
    this.accountRanges.delete(address);
    this.accountHistory.delete(address);
  }

  // Fetch a batch of confirmed signatures but decrease fetch count until
  // the batch is small enough to be queried for statuses.
  async fetchConfirmedSignatureBatch(
    pubkey: PublicKey,
    start: number,
    fetchCount: number,
    forward: boolean
  ): Promise<{
    batch: Array<TransactionSignature>;
    batchRange: SlotRange;
  }> {
    const fullRange = await this.fullRange(false);
    const nextRange = (): SlotRange => {
      if (forward) {
        return {
          min: start,
          max: Math.min(fullRange.max, start + fetchCount - 1),
        };
      } else {
        return {
          min: Math.max(fullRange.min, start - fetchCount + 1),
          max: start,
        };
      }
    };

    let batch: TransactionSignature[] = [];
    let batchRange = nextRange();
    while (batchRange.max > batchRange.min) {
      batch = await this.connection.getConfirmedSignaturesForAddress(
        pubkey,
        batchRange.min,
        batchRange.max
      );

      // Fetched too many results, refetch with a smaller range (1/8)
      if (batch.length > 4 * MAX_STATUS_BATCH_SIZE) {
        fetchCount = Math.ceil(fetchCount / 8);
        batchRange = nextRange();
      } else {
        batch = batch.reverse();
        break;
      }
    }

    return { batchRange, batch };
  }

  async fetchAccountHistory(pubkey: PublicKey, searchForward: boolean) {
    const address = pubkey.toBase58();

    if (this.accountLock.get(address) === true) return;
    this.accountLock.set(address, true);

    try {
      // Start with only 250 slots in case queried account is a vote account
      let slotFetchCount = 250;
      const fullRange = await this.fullRange(searchForward);
      const currentRange = this.accountRanges.get(address);

      // Determine query range based on already queried range
      let startSlot: number;
      if (currentRange) {
        if (searchForward) {
          startSlot = currentRange.max + 1;
        } else {
          startSlot = currentRange.min - 1;
        }
      } else {
        searchForward = false;
        startSlot = fullRange.max;
      }

      // Gradually fetch more history if not too many signatures were found
      let signatures: string[] = [];
      let range: SlotRange = { min: startSlot, max: startSlot };
      for (var i = 0; i < 5; i++) {
        const { batch, batchRange } = await this.fetchConfirmedSignatureBatch(
          pubkey,
          startSlot,
          slotFetchCount,
          searchForward
        );

        range.min = Math.min(range.min, batchRange.min);
        range.max = Math.max(range.max, batchRange.max);

        if (searchForward) {
          signatures = batch.concat(signatures);
          startSlot = batchRange.max + 1;
        } else {
          signatures = signatures.concat(batch);
          startSlot = batchRange.min - 1;
        }

        if (signatures.length > MAX_STATUS_BATCH_SIZE / 2) break;
        if (range.min <= fullRange.min) break;

        // Bump look-back not that we know the account is probably not a vote account
        slotFetchCount = 10000;
      }

      // Fetch the statuses for all confirmed signatures
      const transactions: HistoricalTransaction[] = [];
      while (signatures.length > 0) {
        const batch = signatures.splice(0, MAX_STATUS_BATCH_SIZE);
        const statuses = (
          await this.connection.getSignatureStatuses(batch, {
            searchTransactionHistory: true,
          })
        ).value;
        statuses.forEach((status, index) => {
          if (status !== null) {
            transactions.push({
              signature: batch[index],
              status,
            });
          }
        });
      }

      // Check if account lock is still active
      if (this.accountLock.get(address) !== true) return;

      // Update fetched slot range
      if (currentRange) {
        currentRange.max = Math.max(range.max, currentRange.max);
        currentRange.min = Math.min(range.min, currentRange.min);
      } else {
        this.accountRanges.set(address, range);
      }

      // Exit early if no new confirmed transactions were found
      const currentTransactions = this.accountHistory.get(address) || [];
      if (currentTransactions.length > 0 && transactions.length === 0) return;

      // Append / prepend newly fetched statuses
      let newTransactions;
      if (searchForward) {
        newTransactions = transactions.concat(currentTransactions);
      } else {
        newTransactions = currentTransactions.concat(transactions);
      }

      this.accountHistory.set(address, newTransactions);
    } finally {
      this.accountLock.set(address, false);
    }
  }
}
