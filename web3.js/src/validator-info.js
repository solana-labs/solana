// @flow

import {struct} from 'superstruct';

import * as Layout from './layout';
import * as shortvec from './util/shortvec-encoding';
import {PublicKey} from './publickey';

export const VALIDATOR_INFO_KEY = new PublicKey(
  'Va1idator1nfo111111111111111111111111111111',
);

/**
 * @private
 */
type ConfigKey = {|
  publicKey: PublicKey,
  isSigner: boolean,
|};

/**
 * Info used to identity validators.
 *
 * @typedef {Object} Info
 * @property {string} name validator name
 * @property {?string} website optional, validator website
 * @property {?string} details optional, extra information the validator chose to share
 * @property {?string} keybaseUsername optional, used to identify validators on keybase.io
 */
export type Info = {|
  name: string,
  website?: string,
  details?: string,
  keybaseUsername?: string,
|};

const InfoString = struct({
  name: 'string',
  website: 'string?',
  details: 'string?',
  keybaseUsername: 'string?',
});

/**
 * ValidatorInfo class
 */
export class ValidatorInfo {
  /**
   * validator public key
   */
  key: PublicKey;
  /**
   * validator information
   */
  info: Info;

  /**
   * Construct a valid ValidatorInfo
   *
   * @param key validator public key
   * @param info validator information
   */
  constructor(key: PublicKey, info: Info) {
    this.key = key;
    this.info = info;
  }

  /**
   * Deserialize ValidatorInfo from the config account data. Exactly two config
   * keys are required in the data.
   *
   * @param buffer config account data
   * @return null if info was not found
   */
  static fromConfigData(
    buffer: Buffer | Uint8Array | Array<number>,
  ): ValidatorInfo | null {
    const PUBKEY_LENGTH = 32;

    let byteArray = [...buffer];
    const configKeyCount = shortvec.decodeLength(byteArray);
    if (configKeyCount !== 2) return null;

    const configKeys: Array<ConfigKey> = [];
    for (let i = 0; i < 2; i++) {
      const publicKey = new PublicKey(byteArray.slice(0, PUBKEY_LENGTH));
      byteArray = byteArray.slice(PUBKEY_LENGTH);
      const isSigner = byteArray.slice(0, 1)[0] === 1;
      byteArray = byteArray.slice(1);
      configKeys.push({publicKey, isSigner});
    }

    if (configKeys[0].publicKey.equals(VALIDATOR_INFO_KEY)) {
      if (configKeys[1].isSigner) {
        const rawInfo = Layout.rustString().decode(Buffer.from(byteArray));
        const info = InfoString(JSON.parse(rawInfo));
        return new ValidatorInfo(configKeys[1].publicKey, info);
      }
    }

    return null;
  }
}
