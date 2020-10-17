import {
  Secp256k1Program,
  Secp256k1Instruction,
  constructEthPubkey,
} from '../src/secp256k1-program';
import {privateKeyVerify, ecdsaSign, publicKeyCreate} from 'secp256k1';
import createKeccakHash from 'keccak';
import {randomBytes} from 'crypto';
import {mockRpcEnabled} from './__mocks__/node-fetch';
import {url} from './url';

import {
  Connection,
  Account,
  sendAndConfirmTransaction,
  LAMPORTS_PER_SOL,
  Transaction,
} from '../src';

if (!mockRpcEnabled) {
  jest.setTimeout(20000);
}

test('decode secp256k1 instruction', () => {
  const program = new Secp256k1Program();

  let privateKey;
  do {
    privateKey = randomBytes(32);
  } while (!privateKeyVerify(privateKey));

  const instruction = program.createSecpInstructionWithPrivateKey({
    privateKey,
    message: Buffer.from('Test message'),
  });

  let {
    signature,
    ethPublicKey,
    recoveryId,
    message,
  } = Secp256k1Instruction.decodeInstruction(instruction);

  expect(recoveryId > -1).toEqual(true);
  expect(signature.length).toEqual(64);
  expect(ethPublicKey.length).toEqual(20);
  expect(message.toString('utf8')).toEqual('Test message');
});

test('live create secp256k1 instruction with public key', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const program = new Secp256k1Program();
  const message = Buffer.from('This is a message');

  let privateKey;
  do {
    privateKey = randomBytes(32);
  } while (!privateKeyVerify(privateKey));

  const publicKey = publicKeyCreate(privateKey, false);
  const messageHash = createKeccakHash('keccak256').update(message).digest();
  const {signature, recid: recoveryId} = ecdsaSign(messageHash, privateKey);

  const instruction = program.createSecpInstructionWithPublicKey({
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

  const transactionSignature = await sendAndConfirmTransaction(
    connection,
    transaction,
    [from],
    {
      commitment: 'single',
      skipPreflight: true,
    },
  );

  expect(transactionSignature.length > 0).toEqual(true);
});

test('live create secp256k1 instruction with private key', async () => {
  if (mockRpcEnabled) {
    console.log('non-live test skipped');
    return;
  }

  const program = new Secp256k1Program();

  let privateKey;
  do {
    privateKey = randomBytes(32);
  } while (!privateKeyVerify(privateKey));

  const instruction = program.createSecpInstructionWithPrivateKey({
    privateKey,
    message: Buffer.from('Test 123'),
  });

  const transaction = new Transaction();
  transaction.add(instruction);

  const connection = new Connection(url, 'recent');
  const from = new Account();
  await connection.requestAirdrop(from.publicKey, 2 * LAMPORTS_PER_SOL);

  const signature = await sendAndConfirmTransaction(
    connection,
    transaction,
    [from],
    {
      commitment: 'single',
      skipPreflight: true,
    },
  );

  expect(signature.length > 0).toEqual(true);
});
