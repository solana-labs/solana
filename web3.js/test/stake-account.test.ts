import {expect} from 'chai';

import {
  Keypair,
  StakeAccountData,
  stakeAccountLayout,
  StakeStateType,
} from '../src';
import {toBuffer} from '../src/utils/to-buffer';

describe('Stake Account', () => {
  it('stake account layout', () => {
    // Layout when StakeState is Stake
    const ExpectedDataStake: StakeAccountData = {
      stateType: StakeStateType.STAKE,
      state: {
        meta: {
          rentExemptReserve: 1,
          lockup: {
            epoch: 32,
            unixTimestamp: 2,
            custodian: toBuffer(Keypair.generate().publicKey.toBuffer()),
          },
          authorized: {
            staker: toBuffer(Keypair.generate().publicKey.toBuffer()),
            withdrawer: toBuffer(Keypair.generate().publicKey.toBuffer()),
          },
        },
        stake: {
          delegation: {
            voterPubkey: toBuffer(Keypair.generate().publicKey.toBuffer()),
            stake: 0,
            activationEpoch: 1,
            deactivationEpoch: 1,
            warmupCooldownRate: 1.2,
          },
          creditsObserved: 1,
        },
      },
    };

    let encodeLayout = stakeAccountLayout(StakeStateType.STAKE);
    let encodedData = Buffer.alloc(encodeLayout.span);
    encodeLayout.encode(ExpectedDataStake, encodedData);
    let decodedLayout = stakeAccountLayout(encodedData);
    let decodedData = decodedLayout.decode(encodedData);
    expect(decodedData).to.deep.equal(ExpectedDataStake);

    // Layout when StakeState is INITIALIZED

    const ExpectedDataInitialized: StakeAccountData = {
      stateType: StakeStateType.INITIALIZED,
      state: {
        rentExemptReserve: 1,
        lockup: {
          unixTimestamp: 2,
          epoch: 32,
          custodian: toBuffer(Keypair.generate().publicKey.toBuffer()),
        },
        authorized: {
          staker: toBuffer(Keypair.generate().publicKey.toBuffer()),
          withdrawer: toBuffer(Keypair.generate().publicKey.toBuffer()),
        },
      },
    };
    encodeLayout = stakeAccountLayout(StakeStateType.INITIALIZED);
    encodedData = Buffer.alloc(encodeLayout.span);
    encodeLayout.encode(ExpectedDataInitialized, encodedData);
    decodedLayout = stakeAccountLayout(encodedData);
    decodedData = decodedLayout.decode(encodedData);
    expect(decodedData).to.deep.equal(ExpectedDataInitialized);

    // Layout when StakeState is UNINITIALIZED

    const ExpectedDataUninitialized: StakeAccountData = {
      stateType: StakeStateType.UNINITIALIZED,
    };
    encodeLayout = stakeAccountLayout(StakeStateType.UNINITIALIZED);
    encodedData = Buffer.alloc(encodeLayout.span);
    encodeLayout.encode(ExpectedDataUninitialized, encodedData);
    decodedLayout = stakeAccountLayout(encodedData);
    decodedData = decodedLayout.decode(encodedData);
    expect(decodedData).to.deep.equal(ExpectedDataUninitialized);
  });
});
