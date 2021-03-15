import {Buffer} from 'buffer';
import {keccak_256} from 'js-sha3';
import {privateKeyVerify, ecdsaSign, publicKeyCreate} from 'secp256k1';

import {
  Connection,
  Account,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  Transaction,
  Secp256k1Program,
} from '../src';
import {url} from './url';

const randomPrivateKey = () => {
  let privateKey;
  do {
    privateKey = new Account().secretKey.slice(0, 32);
  } while (!privateKeyVerify(privateKey));
  return privateKey;
};

if (process.env.TEST_LIVE) {
  describe('secp256k1', () => {
    const privateKey = randomPrivateKey();
    const publicKey = publicKeyCreate(privateKey, false).slice(1);
    const ethAddress = Secp256k1Program.publicKeyToEthAddress(publicKey);
    const from = new Account();
    const connection = new Connection(url, 'confirmed');

    before(async function () {
      await connection.confirmTransaction(
        await connection.requestAirdrop(from.publicKey, 10 * LAMPORTS_PER_SOL),
      );
    });

    it('create secp256k1 instruction with string address', async () => {
      const message = Buffer.from('string address');
      const messageHash = Buffer.from(keccak_256.update(message).digest());
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);
      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithEthAddress({
          ethAddress: ethAddress.toString('hex'),
          message,
          signature,
          recoveryId,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });

    it('create secp256k1 instruction with 0x prefix string address', async () => {
      const message = Buffer.from('0x string address');
      const messageHash = Buffer.from(keccak_256.update(message).digest());
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);
      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithEthAddress({
          ethAddress: '0x' + ethAddress.toString('hex'),
          message,
          signature,
          recoveryId,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });

    it('create secp256k1 instruction with buffer address', async () => {
      const message = Buffer.from('buffer address');
      const messageHash = Buffer.from(keccak_256.update(message).digest());
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);
      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithEthAddress({
          ethAddress,
          message,
          signature,
          recoveryId,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });

    it('create secp256k1 instruction with public key', async () => {
      const message = Buffer.from('public key');
      const messageHash = Buffer.from(keccak_256.update(message).digest());
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);
      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithPublicKey({
          publicKey,
          message,
          signature,
          recoveryId,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });

    it('create secp256k1 instruction with private key', async () => {
      const message = Buffer.from('private key');
      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithPrivateKey({
          privateKey,
          message,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });
  });
}
