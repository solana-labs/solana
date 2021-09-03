import {Buffer} from 'buffer';
import nacl from 'tweetnacl';

import {
  Connection,
  Keypair,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  Transaction,
  Ed25519Program,
} from '../src';
import {url} from './url';

if (process.env.TEST_LIVE) {
  describe('ed25519', () => {
    const keypair = Keypair.generate();
    const privateKey = keypair.secretKey;
    const publicKey = keypair.publicKey.toBytes();
    const from = Keypair.generate();
    const connection = new Connection(url, 'confirmed');

    before(async function () {
      await connection.confirmTransaction(
        await connection.requestAirdrop(from.publicKey, 10 * LAMPORTS_PER_SOL),
      );
    });

    it('create ed25519 instruction', async () => {
      const message = Buffer.from('string address');
      const signature = nacl.sign.detached(message, privateKey);
      const transaction = new Transaction().add(
        Ed25519Program.createInstructionWithPublicKey({
          publicKey,
          message,
          signature,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });

    it('create ed25519 instruction with private key', async () => {
      const message = Buffer.from('private key');
      const transaction = new Transaction().add(
        Ed25519Program.createInstructionWithPrivateKey({
          privateKey,
          message,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from]);
    });
  });
}
