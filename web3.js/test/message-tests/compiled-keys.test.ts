import {expect} from 'chai';

import {CompiledKeyMeta, CompiledKeys} from '../../src/message/compiled-keys';
import {AddressLookupTableAccount} from '../../src/programs';
import {PublicKey} from '../../src/publickey';
import {AccountMeta, TransactionInstruction} from '../../src/transaction';

function createTestKeys(count: number): Array<PublicKey> {
  return new Array(count).fill(0).map(() => PublicKey.unique());
}

function createTestLookupTable(
  addresses: Array<PublicKey>,
): AddressLookupTableAccount {
  const U64_MAX = 2n ** 64n - 1n;
  return new AddressLookupTableAccount({
    key: PublicKey.unique(),
    state: {
      lastExtendedSlot: 0,
      lastExtendedSlotStartIndex: 0,
      deactivationSlot: U64_MAX,
      authority: PublicKey.unique(),
      addresses,
    },
  });
}

describe('CompiledKeys', () => {
  it('compile', () => {
    const payer = PublicKey.unique();
    const keys = createTestKeys(4);
    const programIds = createTestKeys(4);
    const compiledKeys = CompiledKeys.compile(
      [
        new TransactionInstruction({
          programId: programIds[0],
          keys: [
            createAccountMeta(keys[0], false, false),
            createAccountMeta(keys[1], true, false),
            createAccountMeta(keys[2], false, true),
            createAccountMeta(keys[3], true, true),
            // duplicate the account metas
            createAccountMeta(keys[0], false, false),
            createAccountMeta(keys[1], true, false),
            createAccountMeta(keys[2], false, true),
            createAccountMeta(keys[3], true, true),
            // reference program ids
            createAccountMeta(programIds[0], false, false),
            createAccountMeta(programIds[1], true, false),
            createAccountMeta(programIds[2], false, true),
            createAccountMeta(programIds[3], true, true),
          ],
        }),
        new TransactionInstruction({programId: programIds[1], keys: []}),
        new TransactionInstruction({programId: programIds[2], keys: []}),
        new TransactionInstruction({programId: programIds[3], keys: []}),
      ],
      payer,
    );

    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, payer, true, true, false);
    setMapEntry(map, keys[0], false, false, false);
    setMapEntry(map, keys[1], true, false, false);
    setMapEntry(map, keys[2], false, true, false);
    setMapEntry(map, keys[3], true, true, false);
    setMapEntry(map, programIds[0], false, false, true);
    setMapEntry(map, programIds[1], true, false, true);
    setMapEntry(map, programIds[2], false, true, true);
    setMapEntry(map, programIds[3], true, true, true);
    expect(compiledKeys.keyMetaMap).to.eql(map);
    expect(compiledKeys.payer).to.eq(payer);
  });

  it('compile with dup payer', () => {
    const [payer, programId] = createTestKeys(2);
    const compiledKeys = CompiledKeys.compile(
      [
        new TransactionInstruction({
          programId: programId,
          keys: [createAccountMeta(payer, false, false)],
        }),
      ],
      payer,
    );

    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, payer, true, true, false);
    setMapEntry(map, programId, false, false, true);
    expect(compiledKeys.keyMetaMap).to.eql(map);
    expect(compiledKeys.payer).to.eq(payer);
  });

  it('compile with dup key', () => {
    const [payer, key, programId] = createTestKeys(3);
    const compiledKeys = CompiledKeys.compile(
      [
        new TransactionInstruction({
          programId: programId,
          keys: [
            createAccountMeta(key, false, false),
            createAccountMeta(key, true, true),
          ],
        }),
      ],
      payer,
    );

    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, payer, true, true, false);
    setMapEntry(map, key, true, true, false);
    setMapEntry(map, programId, false, false, true);
    expect(compiledKeys.keyMetaMap).to.eql(map);
    expect(compiledKeys.payer).to.eq(payer);
  });

  it('getMessageComponents', () => {
    const keys = createTestKeys(4);
    const payer = keys[0];
    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, payer, true, true, false);
    setMapEntry(map, keys[1], true, false, false);
    setMapEntry(map, keys[2], false, true, false);
    setMapEntry(map, keys[3], false, false, false);
    const compiledKeys = new CompiledKeys(payer, map);
    const [header, staticAccountKeys] = compiledKeys.getMessageComponents();
    expect(staticAccountKeys).to.eql(keys);
    expect(header).to.eql({
      numRequiredSignatures: 2,
      numReadonlySignedAccounts: 1,
      numReadonlyUnsignedAccounts: 1,
    });
  });

  it('getMessageComponents with overflow', () => {
    const keys = createTestKeys(257);
    const map = new Map<string, CompiledKeyMeta>();
    for (const key of keys) {
      setMapEntry(map, key, true, true, false);
    }
    const compiledKeys = new CompiledKeys(keys[0], map);
    expect(() => compiledKeys.getMessageComponents()).to.throw(
      'Max static account keys length exceeded',
    );
  });

  it('extractTableLookup', () => {
    const keys = createTestKeys(6);
    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, keys[0], true, true, false);
    setMapEntry(map, keys[1], true, false, false);
    setMapEntry(map, keys[2], false, true, false);
    setMapEntry(map, keys[3], false, false, false);
    setMapEntry(map, keys[4], true, false, true);
    setMapEntry(map, keys[5], false, false, true);

    const lookupTable = createTestLookupTable([...keys, ...keys]);
    const compiledKeys = new CompiledKeys(keys[0], map);
    const extractResult = compiledKeys.extractTableLookup(lookupTable);
    if (extractResult === undefined) {
      expect(extractResult).to.not.be.undefined;
      return;
    }

    const [tableLookup, extractedAddresses] = extractResult;
    expect(tableLookup).to.eql({
      accountKey: lookupTable.key,
      writableIndexes: [2],
      readonlyIndexes: [3],
    });
    expect(extractedAddresses).to.eql({
      writable: [keys[2]],
      readonly: [keys[3]],
    });
  });

  it('extractTableLookup no extractable keys found', () => {
    const keys = createTestKeys(6);
    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, keys[0], true, true, false);
    setMapEntry(map, keys[1], true, false, false);
    setMapEntry(map, keys[2], true, true, true);
    setMapEntry(map, keys[3], true, false, true);
    setMapEntry(map, keys[4], false, true, true);
    setMapEntry(map, keys[5], false, false, true);

    const lookupTable = createTestLookupTable(keys);
    const compiledKeys = new CompiledKeys(keys[0], map);
    const extractResult = compiledKeys.extractTableLookup(lookupTable);
    expect(extractResult).to.be.undefined;
  });

  it('extractTableLookup with empty lookup table', () => {
    const keys = createTestKeys(2);
    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, keys[0], true, true, false);
    setMapEntry(map, keys[1], false, false, false);

    const lookupTable = createTestLookupTable([]);
    const compiledKeys = new CompiledKeys(keys[0], map);
    const extractResult = compiledKeys.extractTableLookup(lookupTable);
    expect(extractResult).to.be.undefined;
  });

  it('extractTableLookup with invalid lookup table', () => {
    const keys = createTestKeys(257);
    const map = new Map<string, CompiledKeyMeta>();
    setMapEntry(map, keys[0], true, true, false);
    setMapEntry(map, keys[256], false, false, false);

    const lookupTable = createTestLookupTable(keys);
    const compiledKeys = new CompiledKeys(keys[0], map);
    expect(() => compiledKeys.extractTableLookup(lookupTable)).to.throw(
      'Max lookup table index exceeded',
    );
  });
});

function setMapEntry(
  map: Map<string, CompiledKeyMeta>,
  pubkey: PublicKey,
  isSigner: boolean,
  isWritable: boolean,
  isInvoked: boolean,
) {
  map.set(pubkey.toBase58(), {
    isSigner,
    isWritable,
    isInvoked,
  });
}

function createAccountMeta(
  pubkey: PublicKey,
  isSigner: boolean,
  isWritable: boolean,
): AccountMeta {
  return {
    pubkey,
    isSigner,
    isWritable,
  };
}
