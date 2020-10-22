// @flow

import createKeccakHash from 'keccak';
import secp256k1 from 'secp256k1';
import {randomBytes} from 'crypto';

import {Secp256k1Program} from '../src/secp256k1-program';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';
import {
  Connection,
  Account,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  Transaction,
} from '../src';

const {privateKeyVerify, ecdsaSign, publicKeyCreate} = secp256k1;

if (!mockRpcEnabled) {
  jest.setTimeout(20000);
}

test('live create secp256k1 instruction with public key', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const message = Buffer.from('This is a message');

  let privateKey;
  do {
    privateKey = randomBytes(32);
  } while (!privateKeyVerify(privateKey));

  const publicKey = publicKeyCreate(privateKey, false);
  const messageHash = createKeccakHash('keccak256').update(message).digest();
  const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);

  const instruction = Secp256k1Program.createInstructionWithPublicKey({
    publicKey,
    message,
    signature,
    recoveryId,
  });

  const transaction = new Transaction();
  transaction.add(instruction);

  const connection = new Connection(url, 'recent');
  const from = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);

  await sendAndConfirmTransaction(connection, transaction, [from], {
    commitment: 'single',
    skipPreflight: true,
  });
});

test('live create secp256k1 instruction with private key', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  let privateKey;
  do {
    privateKey = randomBytes(32);
  } while (!privateKeyVerify(privateKey));

  const instruction = Secp256k1Program.createInstructionWithPrivateKey({
    privateKey,
    message: Buffer.from('Test 123'),
  });

  const transaction = new Transaction();
  transaction.add(instruction);

  const connection = new Connection(url, 'recent');
  const from = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);

  await sendAndConfirmTransaction(connection, transaction, [from], {
    commitment: 'single',
    skipPreflight: true,
  });
});
