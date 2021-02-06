// @flow
import {Buffer} from 'buffer';
import {expect} from 'chai';
import nacl from 'tweetnacl';

import {PublicKey} from '../src/publickey';
import {ValidatorInfo} from '../src/validator-info';

describe('ValidatorInfo', () => {
  it('from config account data', () => {
    const keypair = nacl.sign.keyPair.fromSeed(
      Uint8Array.from(Array(32).fill(8)),
    );

    const expectedValidatorInfo = new ValidatorInfo(
      new PublicKey(keypair.publicKey),
      {
        name: 'Validator',
        keybaseUsername: 'validator_id',
      },
    );

    // Config data string steps:
    // 1) Generate a keypair
    // 2) Airdrop lamports to the account
    // 3) Modify the `solana-validator-info` tool
    //   a) Remove the keybase id verification step
    //   b) Print base64 account data in the `get --all` codepath
    //   c) Add `println!("Account data: {:?}", base64::encode(&account.data));`
    // 4) Use modified `solana-validator-info` tool to publish validator info
    // 5) And then use it again to fetch the data! (feel free to trim some A's)
    const configData = Buffer.from(
      'AgdRlwF0SPKsXcI8nrx6x4wKJyV6xhRFjeCk8W+AAAAAABOY9ixtGkV8UbpqS189vS9p/KkyFiGNyJl+QWvRfZPKATUAAAAAAAAAeyJrZXliYXNlVXNlcm5hbWUiOiJ2YWxpZGF0b3JfaWQiLCJuYW1lIjoiVmFsaWRhdG9yIn0',
      'base64',
    );
    const info = ValidatorInfo.fromConfigData(configData);

    expect(info).to.eql(expectedValidatorInfo);
  });
});
