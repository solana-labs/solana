import {Buffer} from 'buffer';
import {
  assert as assertType,
  optional,
  string,
  type as pick,
} from 'superstruct';

import * as Layout from './layout';
import * as shortvec from './utils/shortvec-encoding';
import {PublicKey, PUBLIC_KEY_LENGTH} from './publickey';

export const VALIDATOR_INFO_KEY = new PublicKey(
  'Va1idator1nfo111111111111111111111111111111',
);

/**
 * @internal
 */
type ConfigKey = {
  publicKey: PublicKey;
  isSigner: boolean;
};

/**
 * Info used to identity validators.
 */
export type Info = {
  /** validator name */
  name: string;
  /** optional, validator website */
  website?: string;
  /** optional, extra information the validator chose to share */
  details?: string;
  /** optional, used to identify validators on keybase.io */
  keybaseUsername?: string;
};

const InfoString = pick({
  name: string(),
  website: optional(string()),
  details: optional(string()),
  keybaseUsername: optional(string()),
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
    let byteArray = [...buffer];
    const configKeyCount = shortvec.decodeLength(byteArray);
    if (configKeyCount !== 2) return null;

    const configKeys: Array<ConfigKey> = [];
    for (let i = 0; i < 2; i++) {
      const publicKey = new PublicKey(byteArray.slice(0, PUBLIC_KEY_LENGTH));
      byteArray = byteArray.slice(PUBLIC_KEY_LENGTH);
      const isSigner = byteArray.slice(0, 1)[0] === 1;
      byteArray = byteArray.slice(1);
      configKeys.push({publicKey, isSigner});
    }

    if (configKeys[0].publicKey.equals(VALIDATOR_INFO_KEY)) {
      if (configKeys[1].isSigner) {
        const rawInfo: any = Layout.rustString().decode(Buffer.from(byteArray));
        const info = JSON.parse(rawInfo as string);
        assertType(info, InfoString);
        return new ValidatorInfo(configKeys[1].publicKey, info);
      }
    }

    return null;
  }
}
