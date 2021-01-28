import * as flaggedAccounts from '@solana/solana-flagged-accounts';

export function isFlaggedAccount(address: string) {
  return address in flaggedAccounts;
}
