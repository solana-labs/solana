// @flow

import {Buffer} from 'buffer';
import createKeccakHash from 'keccak';
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
import {helpers} from './mocks/rpc-http';

const randomPrivateKey = () => {
  let privateKey;
  do {
    privateKey = new Account().secretKey.slice(0, 32);
  } while (!privateKeyVerify(privateKey));
  return privateKey;
};

if (process.env.TEST_LIVE) {
  describe('secp256k1', () => {
    it('create secp256k1 instruction with public key', async () => {
      const privateKey = randomPrivateKey();
      const publicKey = publicKeyCreate(privateKey, false);
      const message = Buffer.from('This is a message');
      const messageHash = createKeccakHash('keccak256')
        .update(message)
        .digest();
      const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);
      const connection = new Connection(url, 'singleGossip');

      const from = new Account();
      await connection.confirmTransaction(
        await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL),
        'singleGossip',
      );

      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithPublicKey({
          publicKey,
          message,
          signature,
          recoveryId,
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from], {
        commitment: 'singleGossip',
        preflightCommitment: 'singleGossip',
      });
    });

    it('create secp256k1 instruction with private key', async () => {
      const privateKey = randomPrivateKey();
      const connection = new Connection(url, 'singleGossip');

      const from = new Account();
      await connection.confirmTransaction(
        await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL),
        'singleGossip',
      );

      const transaction = new Transaction().add(
        Secp256k1Program.createInstructionWithPrivateKey({
          privateKey,
          message: Buffer.from('Test 123'),
        }),
      );

      await sendAndConfirmTransaction(connection, transaction, [from], {
        commitment: 'singleGossip',
        preflightCommitment: 'singleGossip',
      });
    });
  });
}
