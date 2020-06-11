/**
 * TEMPORARY
 *
 * Adds the ability cache select transactions from mainnet-beta and return them as
 * if they came from the RPC node itself, until we have a longer transaction history
 */

import { TransactionStatusInfo } from "./index";
import {
  Transaction,
  ConfirmedTransaction,
  Message,
  clusterApiUrl
} from "@solana/web3.js";

export const isCached = (url: string, signature: string): boolean => {
  return url === clusterApiUrl("mainnet-beta") && signature in CACHED_STATUSES;
};

export const CACHED_STATUSES: { [key: string]: TransactionStatusInfo } = {
  uQf4pS38FjRF294QFEXizhYkZFjSR9ZSBvvV6MV5b4VpdfRnK3PY9TWZ2qHMQKtte3XwKVLcWqsTF6wL9NEZMty: {
    slot: 10440804,
    result: { err: null },
    timestamp: 1589212180,
    confirmations: "max"
  },
  DYrfStEEzbV5sftX8LgUa54Nwnc5m5E1731cqBtiiC66TeXgKpfqZEQTuFY3vhHZ2K1BsaFM3X9FqisR28EtZr8: {
    slot: 10451288,
    result: { err: null },
    timestamp: 1589216984,
    confirmations: "max"
  },
  "3bLx2PLpkxCxJA5P7HVe8asFdSWXVAh1DrxfkqWE9bWvPRxXE2hqwj1vuSC858fUw3XAGQcHbJknhtNdxY2sehab": {
    slot: 10516588,
    result: { err: null },
    timestamp: 1589247117,
    confirmations: "max"
  },
  "3fE8xNgyxbwbvA5MX3wM87ahDDgCVEaaMMSa8UCWWNxojaRYBgrQyiKXLSxcryMWb7sEyVLBWyqUaRWnQCroSqjY": {
    slot: 10575124,
    result: { err: null },
    timestamp: 1589274236,
    confirmations: "max"
  },
  "5PWymGjKV7T1oqeqGn139EHFyjNM2dnNhHCUcfD2bmdj8cfF95HpY1uJ84W89c4sJQnmyZxXcYrcjumx2jHUvxZQ": {
    slot: 12447825,
    result: { err: null },
    timestamp: 15901860565,
    confirmations: "max"
  },
  "5K4KuqTTRNtzfpxWiwnkePzGfsa3tBEmpMy7vQFR3KWFAZNVY9tvoSaz1Yt5dKxcgsZPio2EsASVDGbQB1HvirGD": {
    slot: 12450728,
    result: { err: null },
    timestamp: 15901874549,
    confirmations: "max"
  },
  "45pGoC4Rr3fJ1TKrsiRkhHRbdUeX7633XAGVec6XzVdpRbzQgHhe6ZC6Uq164MPWtiqMg7wCkC6Wy3jy2BqsDEKf": {
    slot: 12972684,
    result: { err: null },
    timestamp: 1590432412,
    confirmations: "max"
  }
};

export const CACHED_DETAILS: { [key: string]: ConfirmedTransaction } = {
  uQf4pS38FjRF294QFEXizhYkZFjSR9ZSBvvV6MV5b4VpdfRnK3PY9TWZ2qHMQKtte3XwKVLcWqsTF6wL9NEZMty: {
    meta: null,
    slot: 10440804,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 1
        },
        instructions: [
          { accounts: [0, 1], data: "3Bxs411UBrj8QXUb", programIdIndex: 2 }
        ],
        recentBlockhash: "5Aw8MaMYdYtnfJyyrregWMWGgiMtWZ6GtRzeP6Ufo65Z"
      }),
      [
        "uQf4pS38FjRF294QFEXizhYkZFjSR9ZSBvvV6MV5b4VpdfRnK3PY9TWZ2qHMQKtte3XwKVLcWqsTF6wL9NEZMty"
      ]
    )
  },

  DYrfStEEzbV5sftX8LgUa54Nwnc5m5E1731cqBtiiC66TeXgKpfqZEQTuFY3vhHZ2K1BsaFM3X9FqisR28EtZr8: {
    meta: null,
    slot: 10451288,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 1
        },
        instructions: [
          {
            accounts: [0, 1],
            data: "3Bxs3zwYHuDo723R",
            programIdIndex: 2
          }
        ],
        recentBlockhash: "4hXYcBdfcadcjfWV17ZwMa4MXe8kbZHYHwr3GzfyqunL"
      }),
      [
        "DYrfStEEzbV5sftX8LgUa54Nwnc5m5E1731cqBtiiC66TeXgKpfqZEQTuFY3vhHZ2K1BsaFM3X9FqisR28EtZr8"
      ]
    )
  },

  "3bLx2PLpkxCxJA5P7HVe8asFdSWXVAh1DrxfkqWE9bWvPRxXE2hqwj1vuSC858fUw3XAGQcHbJknhtNdxY2sehab": {
    meta: null,
    slot: 10516588,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 1
        },
        instructions: [
          {
            accounts: [0, 1],
            data: "3Bxs3zwYHuDo723R",
            programIdIndex: 2
          }
        ],
        recentBlockhash: "HSzTGt3PJMeQtFr94gEdeZqTRaBxgS8Wf1zq3MDdNT3L"
      }),
      [
        "3bLx2PLpkxCxJA5P7HVe8asFdSWXVAh1DrxfkqWE9bWvPRxXE2hqwj1vuSC858fUw3XAGQcHbJknhtNdxY2sehab"
      ]
    )
  },

  "3fE8xNgyxbwbvA5MX3wM87ahDDgCVEaaMMSa8UCWWNxojaRYBgrQyiKXLSxcryMWb7sEyVLBWyqUaRWnQCroSqjY": {
    meta: null,
    slot: 10575124,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 1
        },
        instructions: [
          {
            accounts: [0, 1],
            data: "3Bxs3zuKU6mRKSqD",
            programIdIndex: 2
          }
        ],
        recentBlockhash: "6f6TBMhUoypfR5HHnEqC6VoooKxEcNad5W3Sf63j9MSD"
      }),
      [
        "3fE8xNgyxbwbvA5MX3wM87ahDDgCVEaaMMSa8UCWWNxojaRYBgrQyiKXLSxcryMWb7sEyVLBWyqUaRWnQCroSqjY"
      ]
    )
  },

  "5PWymGjKV7T1oqeqGn139EHFyjNM2dnNhHCUcfD2bmdj8cfF95HpY1uJ84W89c4sJQnmyZxXcYrcjumx2jHUvxZQ": {
    meta: null,
    slot: 12447825,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "HCV5dGFJXRrJ3jhDYA4DCeb9TEDTwGGYXtT3wHksu2Zr",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 1
        },
        instructions: [
          {
            accounts: [0, 1],
            data: "3Bxs3zrfhSqZJTR1",
            programIdIndex: 2
          }
        ],
        recentBlockhash: "3HJNFraT7XGAqMrQs83EKwDGB6LpHVwUMQKGaYMNY49E"
      }),
      [
        "5PWymGjKV7T1oqeqGn139EHFyjNM2dnNhHCUcfD2bmdj8cfF95HpY1uJ84W89c4sJQnmyZxXcYrcjumx2jHUvxZQ"
      ]
    )
  },

  "5K4KuqTTRNtzfpxWiwnkePzGfsa3tBEmpMy7vQFR3KWFAZNVY9tvoSaz1Yt5dKxcgsZPio2EsASVDGbQB1HvirGD": {
    meta: null,
    slot: 12450728,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "6yKHERk8rsbmJxvMpPuwPs1ct3hRiP7xaJF2tvnGU6nK",
          "4C6NCcLPUgGuBBkV2dJW96mrptMUCp3RG1ft9rqwjFi9",
          "3o6xgkJ9sTmDeQWyfj3sxwon18fXJB9PV5LDc8sfgR4a",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 2
        },
        instructions: [
          {
            accounts: [1, 2],
            data: "3Bxs3ztRCp3tH1yZ",
            programIdIndex: 3
          }
        ],
        recentBlockhash: "8eXVUNRxrDgpsEuoTWyLay1LUh2djc3Y8cw2owXRN8cU"
      }),
      [
        "5K4KuqTTRNtzfpxWiwnkePzGfsa3tBEmpMy7vQFR3KWFAZNVY9tvoSaz1Yt5dKxcgsZPio2EsASVDGbQB1HvirGD",
        "37tvpG1eAeEBizJPhJvmpC2BY8npwy6K1wrZdNwdRAfWSbkerY3ZwYAPMHbrzoq7tthvWC2qFU28niqLPxbukeXF"
      ]
    )
  },

  "45pGoC4Rr3fJ1TKrsiRkhHRbdUeX7633XAGVec6XzVdpRbzQgHhe6ZC6Uq164MPWtiqMg7wCkC6Wy3jy2BqsDEKf": {
    meta: null,
    slot: 12972684,
    transaction: Transaction.populate(
      new Message({
        accountKeys: [
          "6yKHERk8rsbmJxvMpPuwPs1ct3hRiP7xaJF2tvnGU6nK",
          "3o6xgkJ9sTmDeQWyfj3sxwon18fXJB9PV5LDc8sfgR4a",
          "1nc1nerator11111111111111111111111111111111",
          "11111111111111111111111111111111"
        ],
        header: {
          numReadonlySignedAccounts: 0,
          numReadonlyUnsignedAccounts: 1,
          numRequiredSignatures: 2
        },
        instructions: [
          {
            accounts: [1, 2],
            data: "3Bxs4NNAyLXRbuZZ",
            programIdIndex: 3
          }
        ],
        recentBlockhash: "2xnatNUtSbeMRwi3k4vxPwXxeKFQYVuCNRg2rAgydWVP"
      }),
      [
        "45pGoC4Rr3fJ1TKrsiRkhHRbdUeX7633XAGVec6XzVdpRbzQgHhe6ZC6Uq164MPWtiqMg7wCkC6Wy3jy2BqsDEKf",
        "2E7CDMTssxTYkdetCKVWQv9X2KNDPiuZrT2Y7647PhFEXuAWWxmHJb3ryCmP29ocQ1SNc7VyJjjm4X3jE8xWDmGY"
      ]
    )
  }
};
