/* tslint:disable */
/* eslint-disable */
/**
*/
export enum StakeState {
  Uninitialized,
  Initialized,
  Delegated,
  RewardsPool,
}
/**
*/
export class Authorized {
  free(): void;
/**
* @returns {Pubkey} 
*/
  staker: Pubkey;
/**
* @returns {Pubkey} 
*/
  withdrawer: Pubkey;
}
/**
*/
export class Delegation {
  free(): void;
/**
* @returns {boolean} 
*/
  isBootstrapStake(): boolean;
/**
* @returns {boolean} 
*/
  isDeactivated(): boolean;
/**
* @returns {number} 
*/
  readonly activationEpoch: number;
/**
* @returns {number} 
*/
  readonly deactivationEpoch: number;
/**
* @returns {number} 
*/
  readonly stake: number;
/**
* @returns {Pubkey} 
*/
  readonly voterPubkey: Pubkey;
/**
* @returns {number} 
*/
  readonly warmupCooldownRate: number;
}
/**
*/
export class Lockup {
  free(): void;
/**
* custodian signature on a transaction exempts the operation from
*  lockup constraints
* @returns {Pubkey} 
*/
  custodian: Pubkey;
/**
* @returns {number} 
*/
  readonly epoch: number;
/**
* @returns {number} 
*/
  readonly unixTimestamp: number;
}
/**
*/
export class Meta {
  free(): void;
/**
* @returns {Authorized} 
*/
  authorized: Authorized;
/**
* @returns {Lockup} 
*/
  lockup: Lockup;
/**
* @returns {number} 
*/
  readonly rentExemptReserve: number;
}
/**
*/
export class Pubkey {
  free(): void;
/**
* @returns {string} 
*/
  toBase58(): string;
}
/**
*/
export class Stake {
  free(): void;
/**
* @returns {number} 
*/
  readonly creditsObserved: number;
/**
* @returns {Delegation} 
*/
  delegation: Delegation;
}
/**
*/
export class StakeAccount {
  free(): void;
/**
* @param {Uint8Array} data 
* @returns {StakeAccount} 
*/
  static fromAccountData(data: Uint8Array): StakeAccount;
/**
* @returns {string} 
*/
  displayState(): string;
/**
* @returns {Meta | undefined} 
*/
  meta?: Meta;
/**
* @returns {Stake | undefined} 
*/
  stake?: Stake;
/**
* @returns {number} 
*/
  state: number;
}
