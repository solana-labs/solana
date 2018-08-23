import nacl from 'tweetnacl';
import bs58 from 'bs58';

export class Account {
  constructor(secretKey: ?Buffer = null) {
    if (secretKey) {
      this._keypair = nacl.sign.keyPair.fromSecretKey(secretKey);
    } else {
      this._keypair = nacl.sign.keyPair();
    }
  }

  get publicKey(): string {
    return bs58.encode(this._keypair.publicKey);
  }

  get secretKey(): string {
    return this._keypair.secretKey;
  }
}

