// @flow
import bs58 from 'bs58';
import nacl from 'tweetnacl';

import {Account} from '../src/account';
import {PublicKey} from '../src/publickey';
import {Transaction} from '../src/transaction';
import {StakeProgram} from '../src/stake-program';
import {SystemProgram} from '../src/system-program';
import {Message} from '../src/message';

describe('compileMessage', () => {
  test('accountKeys are ordered', () => {
    const payer = new Account();
    const account2 = new Account();
    const account3 = new Account();
    const recentBlockhash = new Account().publicKey.toBase58();
    const programId = new Account().publicKey;
    const transaction = new Transaction({recentBlockhash}).add({
      keys: [
        {pubkey: account3.publicKey, isSigner: true, isWritable: false},
        {pubkey: payer.publicKey, isSigner: true, isWritable: true},
        {pubkey: account2.publicKey, isSigner: true, isWritable: true},
      ],
      programId,
    });

    transaction.setSigners(
      payer.publicKey,
      account2.publicKey,
      account3.publicKey,
    );

    const message = transaction.compileMessage();
    expect(message.accountKeys[0].equals(payer.publicKey)).toBe(true);
    expect(message.accountKeys[1].equals(account2.publicKey)).toBe(true);
    expect(message.accountKeys[2].equals(account3.publicKey)).toBe(true);
  });

  test('payer is first account meta', () => {
    const payer = new Account();
    const other = new Account();
    const recentBlockhash = new Account().publicKey.toBase58();
    const programId = new Account().publicKey;
    const transaction = new Transaction({recentBlockhash}).add({
      keys: [
        {pubkey: other.publicKey, isSigner: true, isWritable: true},
        {pubkey: payer.publicKey, isSigner: true, isWritable: true},
      ],
      programId,
    });

    transaction.sign(payer, other);
    const message = transaction.compileMessage();
    expect(message.accountKeys[0].equals(payer.publicKey)).toBe(true);
    expect(message.accountKeys[1].equals(other.publicKey)).toBe(true);
    expect(message.header.numRequiredSignatures).toEqual(2);
    expect(message.header.numReadonlySignedAccounts).toEqual(0);
    expect(message.header.numReadonlyUnsignedAccounts).toEqual(1);
  });

  test('validation', () => {
    const payer = new Account();
    const other = new Account();
    const recentBlockhash = new Account().publicKey.toBase58();
    const programId = new Account().publicKey;

    const transaction = new Transaction();
    expect(() => {
      transaction.compileMessage();
    }).toThrow('Transaction recentBlockhash required');

    transaction.recentBlockhash = recentBlockhash;

    expect(() => {
      transaction.compileMessage();
    }).toThrow('No instructions provided');

    transaction.add({
      keys: [
        {pubkey: other.publicKey, isSigner: true, isWritable: true},
        {pubkey: payer.publicKey, isSigner: true, isWritable: true},
      ],
      programId,
    });

    expect(() => {
      transaction.compileMessage();
    }).toThrow('Transaction feePayer required');

    transaction.setSigners(payer.publicKey);

    expect(() => {
      transaction.compileMessage();
    }).toThrow('missing signer');

    transaction.setSigners(payer.publicKey, new Account().publicKey);

    expect(() => {
      transaction.compileMessage();
    }).toThrow('unknown signer');
  });

  test('payer is writable', () => {
    const payer = new Account();
    const recentBlockhash = new Account().publicKey.toBase58();
    const programId = new Account().publicKey;
    const transaction = new Transaction({recentBlockhash}).add({
      keys: [{pubkey: payer.publicKey, isSigner: true, isWritable: false}],
      programId,
    });

    transaction.sign(payer);
    const message = transaction.compileMessage();
    expect(message.accountKeys[0].equals(payer.publicKey)).toBe(true);
    expect(message.header.numRequiredSignatures).toEqual(1);
    expect(message.header.numReadonlySignedAccounts).toEqual(0);
    expect(message.header.numReadonlyUnsignedAccounts).toEqual(1);
  });
});

test('partialSign', () => {
  const account1 = new Account();
  const account2 = new Account();
  const recentBlockhash = account1.publicKey.toBase58(); // Fake recentBlockhash
  const transfer = SystemProgram.transfer({
    fromPubkey: account1.publicKey,
    toPubkey: account2.publicKey,
    lamports: 123,
  });

  const transaction = new Transaction({recentBlockhash}).add(transfer);
  transaction.sign(account1, account2);

  const partialTransaction = new Transaction({recentBlockhash}).add(transfer);
  partialTransaction.setSigners(account1.publicKey, account2.publicKey);
  expect(partialTransaction.signatures[0].signature).toBeNull();
  expect(partialTransaction.signatures[1].signature).toBeNull();
  partialTransaction.partialSign(account1, account2);

  expect(partialTransaction).toEqual(transaction);
});

describe('dedupe', () => {
  const payer = new Account();
  const duplicate1 = payer;
  const duplicate2 = payer;
  const recentBlockhash = new Account().publicKey.toBase58();
  const programId = new Account().publicKey;

  test('setSigners', () => {
    const transaction = new Transaction({recentBlockhash}).add({
      keys: [
        {pubkey: duplicate1.publicKey, isSigner: true, isWritable: true},
        {pubkey: payer.publicKey, isSigner: false, isWritable: true},
        {pubkey: duplicate2.publicKey, isSigner: true, isWritable: false},
      ],
      programId,
    });

    transaction.setSigners(
      payer.publicKey,
      duplicate1.publicKey,
      duplicate2.publicKey,
    );

    expect(transaction.signatures.length).toEqual(1);
    expect(transaction.signatures[0].publicKey.equals(payer.publicKey)).toBe(
      true,
    );

    const message = transaction.compileMessage();
    expect(message.accountKeys[0].equals(payer.publicKey)).toBe(true);
    expect(message.header.numRequiredSignatures).toEqual(1);
    expect(message.header.numReadonlySignedAccounts).toEqual(0);
    expect(message.header.numReadonlyUnsignedAccounts).toEqual(1);

    transaction.signatures;
  });

  test('sign', () => {
    const transaction = new Transaction({recentBlockhash}).add({
      keys: [
        {pubkey: duplicate1.publicKey, isSigner: true, isWritable: true},
        {pubkey: payer.publicKey, isSigner: false, isWritable: true},
        {pubkey: duplicate2.publicKey, isSigner: true, isWritable: false},
      ],
      programId,
    });

    transaction.sign(payer, duplicate1, duplicate2);

    expect(transaction.signatures.length).toEqual(1);
    expect(transaction.signatures[0].publicKey.equals(payer.publicKey)).toBe(
      true,
    );

    const message = transaction.compileMessage();
    expect(message.accountKeys[0].equals(payer.publicKey)).toBe(true);
    expect(message.header.numRequiredSignatures).toEqual(1);
    expect(message.header.numReadonlySignedAccounts).toEqual(0);
    expect(message.header.numReadonlyUnsignedAccounts).toEqual(1);

    transaction.signatures;
  });
});

test('transfer signatures', () => {
  const account1 = new Account();
  const account2 = new Account();
  const recentBlockhash = account1.publicKey.toBase58(); // Fake recentBlockhash
  const transfer1 = SystemProgram.transfer({
    fromPubkey: account1.publicKey,
    toPubkey: account2.publicKey,
    lamports: 123,
  });
  const transfer2 = SystemProgram.transfer({
    fromPubkey: account2.publicKey,
    toPubkey: account1.publicKey,
    lamports: 123,
  });

  const orgTransaction = new Transaction({recentBlockhash}).add(
    transfer1,
    transfer2,
  );
  orgTransaction.sign(account1, account2);

  const newTransaction = new Transaction({
    recentBlockhash: orgTransaction.recentBlockhash,
    signatures: orgTransaction.signatures,
  }).add(transfer1, transfer2);

  expect(newTransaction).toEqual(orgTransaction);
});

test('dedup signatures', () => {
  const account1 = new Account();
  const account2 = new Account();
  const recentBlockhash = account1.publicKey.toBase58(); // Fake recentBlockhash
  const transfer1 = SystemProgram.transfer({
    fromPubkey: account1.publicKey,
    toPubkey: account2.publicKey,
    lamports: 123,
  });
  const transfer2 = SystemProgram.transfer({
    fromPubkey: account1.publicKey,
    toPubkey: account2.publicKey,
    lamports: 123,
  });

  const orgTransaction = new Transaction({recentBlockhash}).add(
    transfer1,
    transfer2,
  );
  orgTransaction.sign(account1);
});

test('use nonce', () => {
  const account1 = new Account();
  const account2 = new Account();
  const nonceAccount = new Account();
  const nonce = account2.publicKey.toBase58(); // Fake Nonce hash

  const nonceInfo = {
    nonce,
    nonceInstruction: SystemProgram.nonceAdvance({
      noncePubkey: nonceAccount.publicKey,
      authorizedPubkey: account1.publicKey,
    }),
  };

  const transferTransaction = new Transaction({nonceInfo}).add(
    SystemProgram.transfer({
      fromPubkey: account1.publicKey,
      toPubkey: account2.publicKey,
      lamports: 123,
    }),
  );
  transferTransaction.sign(account1);

  let expectedData = Buffer.alloc(4);
  expectedData.writeInt32LE(4, 0);

  expect(transferTransaction.instructions).toHaveLength(2);
  expect(transferTransaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(transferTransaction.instructions[0].data).toEqual(expectedData);
  expect(transferTransaction.recentBlockhash).toEqual(nonce);

  const stakeAccount = new Account();
  const voteAccount = new Account();
  const stakeTransaction = new Transaction({nonceInfo}).add(
    StakeProgram.delegate({
      stakePubkey: stakeAccount.publicKey,
      authorizedPubkey: account1.publicKey,
      votePubkey: voteAccount.publicKey,
    }),
  );
  stakeTransaction.sign(account1);

  expect(stakeTransaction.instructions).toHaveLength(2);
  expect(stakeTransaction.instructions[0].programId).toEqual(
    SystemProgram.programId,
  );
  expect(stakeTransaction.instructions[0].data).toEqual(expectedData);
  expect(stakeTransaction.recentBlockhash).toEqual(nonce);
});

test('parse wire format and serialize', () => {
  const keypair = nacl.sign.keyPair.fromSeed(
    Uint8Array.from(Array(32).fill(8)),
  );
  const sender = new Account(Buffer.from(keypair.secretKey)); // Arbitrary known account
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const recipient = new PublicKey(
    'J3dxNj7nDRRqRRXuEMynDG57DkZK4jYRuv3Garmb1i99',
  ); // Arbitrary known public key
  const transfer = SystemProgram.transfer({
    fromPubkey: sender.publicKey,
    toPubkey: recipient,
    lamports: 49,
  });
  const expectedTransaction = new Transaction({recentBlockhash}).add(transfer);
  expectedTransaction.sign(sender);

  const wireTransaction = Buffer.from(
    'AVuErQHaXv0SG0/PchunfxHKt8wMRfMZzqV0tkC5qO6owYxWU2v871AoWywGoFQr4z+q/7mE8lIufNl/kxj+nQ0BAAEDE5j2LG0aRXxRumpLXz29L2n8qTIWIY3ImX5Ba9F9k8r9Q5/Mtmcn8onFxt47xKj+XdXXd3C8j/FcPu7csUrz/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxJrndgN4IFTxep3s6kO0ROug7bEsbx0xxuDkqEvwUusBAgIAAQwCAAAAMQAAAAAAAAA=',
    'base64',
  );
  const tx = Transaction.from(wireTransaction);

  expect(tx).toEqual(expectedTransaction);
  expect(wireTransaction).toEqual(expectedTransaction.serialize());
});

test('populate transaction', () => {
  const recentBlockhash = new PublicKey(1).toString();
  const message = {
    accountKeys: [
      new PublicKey(1).toString(),
      new PublicKey(2).toString(),
      new PublicKey(3).toString(),
      new PublicKey(4).toString(),
      new PublicKey(5).toString(),
    ],
    header: {
      numReadonlySignedAccounts: 0,
      numReadonlyUnsignedAccounts: 3,
      numRequiredSignatures: 2,
    },
    instructions: [
      {
        accounts: [1, 2, 3],
        data: bs58.encode(Buffer.alloc(5).fill(9)),
        programIdIndex: 4,
      },
    ],
    recentBlockhash,
  };

  const signatures = [
    bs58.encode(Buffer.alloc(64).fill(1)),
    bs58.encode(Buffer.alloc(64).fill(2)),
  ];

  const transaction = Transaction.populate(new Message(message), signatures);
  expect(transaction.instructions.length).toEqual(1);
  expect(transaction.signatures.length).toEqual(2);
  expect(transaction.recentBlockhash).toEqual(recentBlockhash);
});

test('serialize unsigned transaction', () => {
  const keypair = nacl.sign.keyPair.fromSeed(
    Uint8Array.from(Array(32).fill(8)),
  );
  const sender = new Account(Buffer.from(keypair.secretKey)); // Arbitrary known account
  const recentBlockhash = 'EETubP5AKHgjPAhzPAFcb8BAY1hMH639CWCFTqi3hq1k'; // Arbitrary known recentBlockhash
  const recipient = new PublicKey(
    'J3dxNj7nDRRqRRXuEMynDG57DkZK4jYRuv3Garmb1i99',
  ); // Arbitrary known public key
  const transfer = SystemProgram.transfer({
    fromPubkey: sender.publicKey,
    toPubkey: recipient,
    lamports: 49,
  });
  const expectedTransaction = new Transaction({recentBlockhash}).add(transfer);

  // Empty signature array fails.
  expect(expectedTransaction.signatures.length).toBe(0);
  expect(() => {
    expectedTransaction.serialize();
  }).toThrow(Error);
  expect(() => {
    expectedTransaction.serializeMessage();
  }).toThrow('Transaction feePayer required');

  expectedTransaction.setSigners(sender.publicKey);
  expect(expectedTransaction.signatures.length).toBe(1);

  // Signature array populated with null signatures fails.
  expect(() => {
    expectedTransaction.serialize();
  }).toThrow(Error);

  // Serializing the message is allowed when signature array has null signatures
  expectedTransaction.serializeMessage();

  // Properly signed transaction succeeds
  expectedTransaction.partialSign(sender);
  expect(expectedTransaction.signatures.length).toBe(1);
  const expectedSerialization = Buffer.from(
    'AVuErQHaXv0SG0/PchunfxHKt8wMRfMZzqV0tkC5qO6owYxWU2v871AoWywGoFQr4z+q/7mE8lIufNl/' +
      'kxj+nQ0BAAEDE5j2LG0aRXxRumpLXz29L2n8qTIWIY3ImX5Ba9F9k8r9Q5/Mtmcn8onFxt47xKj+XdXX' +
      'd3C8j/FcPu7csUrz/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxJrndgN4IFTxep3s6kO0' +
      'ROug7bEsbx0xxuDkqEvwUusBAgIAAQwCAAAAMQAAAAAAAAA=',
    'base64',
  );
  expect(expectedTransaction.serialize()).toStrictEqual(expectedSerialization);
  expect(expectedTransaction.signatures.length).toBe(1);
});

test('externally signed stake delegate', () => {
  const from_keypair = nacl.sign.keyPair.fromSeed(
    Uint8Array.from(Array(32).fill(1)),
  );
  const authority = new Account(Buffer.from(from_keypair.secretKey));
  const stake = new PublicKey(2);
  const recentBlockhash = new PublicKey(3).toBuffer();
  const vote = new PublicKey(4);
  var tx = StakeProgram.delegate({
    stakePubkey: stake,
    authorizedPubkey: authority.publicKey,
    votePubkey: vote,
  });
  const from = authority;
  tx.recentBlockhash = bs58.encode(recentBlockhash);
  tx.setSigners(from.publicKey);
  const tx_bytes = tx.serializeMessage();
  const signature = nacl.sign.detached(tx_bytes, from.secretKey);
  tx.addSignature(from.publicKey, signature);
  expect(tx.verifySignatures()).toBe(true);
});
