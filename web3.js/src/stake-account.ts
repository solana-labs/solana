import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from './layout';

/** Define who is authorized to change a stake */
export type AuthorizedRaw = Readonly<{
  staker: Uint8Array;
  withdrawer: Uint8Array;
}>;

export type LockupRaw = Readonly<{
  /** custodian can update the lockup while in force */
  custodian: Uint8Array;
  epoch: number;
  unixTimestamp: number;
}>;

/** Stake State  When Statetype is INITIALIZED */
export type Meta = Readonly<{
  rentExemptReserve: number;
  authorized: AuthorizedRaw;
  lockup: LockupRaw;
}>;

export type Delegation = Readonly<{
  /** to whom the stake is delegated */
  voterPubkey: Uint8Array;
  /** activated stake amount, set at delegate() time */
  stake: number;
  /** epoch at which this stake was activated, std::Epoch::MAX if is a bootstrap stake */
  activationEpoch: number;
  /** epoch the stake was deactivated, std::Epoch::MAX if not deactivated */
  deactivationEpoch: number;
  /** how much stake we can activate per-epoch as a fraction of currently effective stake */
  warmupCooldownRate: number;
}>;

export type Stake = Readonly<{
  delegation: Delegation;
  /** credits observed is credits from vote account state when delegated or redeemed */
  creditsObserved: number;
}>;

export type StakeAndMeta = Readonly<{
  /** Stake State  When Statetype is STAKE */
  meta: Meta;
  stake: Stake;
}>;

export enum StakeStateType {
  /** Stake State Types */
  UNINITIALIZED = 0,
  INITIALIZED = 1,
  STAKE = 2,
  REWARDS_POOL = 3,
}

export type StakeAccountData = {
  stateType: StakeStateType;
  /** Stake State */
  state?: StakeAndMeta | Meta;
};

/**
 * See https://github.com/solana-labs/solana/blob/f7c6901191cb3eac8c876362c19159a9d8feec95/sdk/program/src/stake/state.rs#L21
 *
 * @internal
 */

export function stakeAccountLayout(
  buffer: Buffer | number,
): BufferLayout.Layout<StakeAccountData> {
  let type;
  if (typeof buffer === 'number') {
    type = buffer;
  } else {
    const layout = BufferLayout.struct<StakeAccountData>([
      BufferLayout.u32('stateType'),
    ]);
    const decodedData = layout.decode(buffer);
    type = decodedData.stateType;
  }
  switch (type) {
    case StakeStateType.INITIALIZED:
      return BufferLayout.struct<StakeAccountData>([
        BufferLayout.u32('stateType'),
        BufferLayout.struct<Meta>(
          [
            BufferLayout.nu64('rentExemptReserve'),
            Layout.lockup(),
            Layout.authorized(),
          ],
          'state',
        ),
      ]);
    case StakeStateType.STAKE:
      return BufferLayout.struct<StakeAccountData>([
        BufferLayout.u32('stateType'),
        BufferLayout.struct<StakeAndMeta>(
          [
            BufferLayout.struct<Meta>(
              [
                BufferLayout.nu64('rentExemptReserve'),
                Layout.lockup(),
                Layout.authorized(),
              ],
              'meta',
            ),
            BufferLayout.struct<Stake>(
              [
                BufferLayout.struct<Delegation>(
                  [
                    Layout.publicKey('voterPubkey'),
                    BufferLayout.nu64('stake'),
                    BufferLayout.nu64('activationEpoch'),
                    BufferLayout.nu64('deactivationEpoch'),
                    BufferLayout.f64('warmupCooldownRate'),
                  ],
                  'delegation',
                ),
                BufferLayout.nu64('creditsObserved'),
              ],
              'stake',
            ),
          ],
          'state',
        ),
      ]);
    default:
      return BufferLayout.struct<StakeAccountData>([
        BufferLayout.u32('stateType'),
      ]);
  }
}
