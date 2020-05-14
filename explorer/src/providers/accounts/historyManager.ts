import {
  TransactionSignature,
  Connection,
  PublicKey,
  SignatureStatus
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

  async fetchAccountHistory(pubkey: PublicKey, refresh: boolean) {
    const address = pubkey.toBase58();

    if (this.accountLock.get(address) === true) return;
    this.accountLock.set(address, true);

    try {
      let slotLookBack = 100;
      const fullRange = await this.fullRange(refresh);
      const currentRange = this.accountRanges.get(address);

      // Determine query range based on already queried range
      let range;
      if (currentRange) {
        if (refresh) {
          const min = currentRange.max + 1;
          const max = Math.min(min + slotLookBack - 1, fullRange.max);
          if (max < min) return;
          range = { min, max };
        } else {
          const max = currentRange.min - 1;
          const min = Math.max(max - slotLookBack + 1, fullRange.min);
          if (max < min) return;
          range = { min, max };
        }
      } else {
        const max = fullRange.max;
        const min = Math.max(fullRange.min, max - slotLookBack + 1);
        range = { min, max };
      }

      // Gradually fetch more history if nothing found
      let signatures: string[] = [];
      let nextRange = { ...range };
      for (var i = 0; i < 5; i++) {
        signatures = (
          await this.connection.getConfirmedSignaturesForAddress(
            pubkey,
            nextRange.min,
            nextRange.max
          )
        ).reverse();
        if (refresh) break;
        if (signatures.length > 0) break;
        if (range.min <= fullRange.min) break;

        switch (slotLookBack) {
          case 100:
            slotLookBack = 1000;
            break;
          case 1000:
            slotLookBack = 10000;
            break;
        }

        range.min = Math.max(nextRange.min - slotLookBack, fullRange.min);
        nextRange = {
          min: range.min,
          max: nextRange.min - 1
        };
      }

      // Fetch the statuses for all confirmed signatures
      const transactions: HistoricalTransaction[] = [];
      while (signatures.length > 0) {
        const batch = signatures.splice(0, MAX_STATUS_BATCH_SIZE);
        const statuses = (
          await this.connection.getSignatureStatuses(batch, {
            searchTransactionHistory: true
          })
        ).value;
        statuses.forEach((status, index) => {
          if (status !== null) {
            transactions.push({
              signature: batch[index],
              status
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
      if (refresh) {
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
