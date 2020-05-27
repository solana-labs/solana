// @flow
import {PublicKey} from '../src/publickey';

test('invalid', () => {
  expect(() => {
    new PublicKey([
      3,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
      0,
    ]);
  }).toThrow();

  expect(() => {
    new PublicKey(
      '0x300000000000000000000000000000000000000000000000000000000000000000000',
    );
  }).toThrow();

  expect(() => {
    new PublicKey(
      '135693854574979916511997248057056142015550763280047535983739356259273198796800000',
    );
  }).toThrow();

  expect(() => {
    new PublicKey('12345');
  }).toThrow();
});

test('equals', () => {
  const arrayKey = new PublicKey([
    3,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
  ]);
  const hexKey = new PublicKey(
    '0x300000000000000000000000000000000000000000000000000000000000000',
  );
  const base56Key = new PublicKey(
    'CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3',
  );

  expect(arrayKey.equals(hexKey)).toBe(true);
  expect(arrayKey.equals(base56Key)).toBe(true);
});

test('isPublicKey', () => {
  const key = new PublicKey(
    '0x100000000000000000000000000000000000000000000000000000000000000',
  );
  expect(PublicKey.isPublicKey(key)).toBe(true);
  expect(PublicKey.isPublicKey({})).toBe(false);
});

test('toBase58', () => {
  const key = new PublicKey(
    '0x300000000000000000000000000000000000000000000000000000000000000',
  );
  expect(key.toBase58()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
  expect(key.toString()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  const key2 = new PublicKey('1111111111111111111111111111BukQL');
  expect(key2.toBase58()).toBe('1111111111111111111111111111BukQL');
  expect(key2.toString()).toBe('1111111111111111111111111111BukQL');

  const key3 = new PublicKey('11111111111111111111111111111111');
  expect(key3.toBase58()).toBe('11111111111111111111111111111111');

  const key4 = new PublicKey([
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
  ]);
  expect(key4.toBase58()).toBe('11111111111111111111111111111111');
});

test('toBuffer', () => {
  const key = new PublicKey(
    '0x300000000000000000000000000000000000000000000000000000000000000',
  );
  expect(key.toBuffer()).toHaveLength(32);
  expect(key.toBase58()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  const key2 = new PublicKey(
    '0x000000000000000000000000000000000000000000000000000000000000000',
  );
  expect(key2.toBuffer()).toHaveLength(32);
  expect(key2.toBase58()).toBe('11111111111111111111111111111111');

  const key3 = new PublicKey(0);
  expect(key3.toBuffer()).toHaveLength(32);
  expect(key3.toBase58()).toBe('11111111111111111111111111111111');

  const key4 = new PublicKey('0x0');
  expect(key4.toBuffer()).toHaveLength(32);
  expect(key4.toBase58()).toBe('11111111111111111111111111111111');
});

test('equals (II)', () => {
  const key1 = new PublicKey([
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    1,
  ]);
  const key2 = new PublicKey(key1.toBuffer());

  expect(key1.equals(key2)).toBe(true);
});

test('createWithSeed', async () => {
  const defaultPublicKey = new PublicKey('11111111111111111111111111111111');
  const derivedKey = await PublicKey.createWithSeed(
    defaultPublicKey,
    'limber chicken: 4/45',
    defaultPublicKey,
  );

  expect(
    derivedKey.equals(
      new PublicKey('9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq'),
    ),
  ).toBe(true);
});

test('createProgramAddress', async () => {
  const programId = new PublicKey(
    'BPFLoader1111111111111111111111111111111111',
  );

  let programAddress = await PublicKey.createProgramAddress([''], programId);
  expect(
    programAddress.equals(
      new PublicKey('CsdSsqp6Upkh2qajhZMBM8xT4GAyDNSmcV37g4pN8rsc'),
    ),
  ).toBe(true);
  programAddress = await PublicKey.createProgramAddress(['☉'], programId);
  expect(
    programAddress.equals(
      new PublicKey('A8mYnN8Pfx7Nn6f8RoQgsPNtAGAWmmKSBCDfyDvE6sXF'),
    ),
  ).toBe(true);
  programAddress = await PublicKey.createProgramAddress(
    ['Talking', 'Squirrels'],
    programId,
  );
  expect(
    programAddress.equals(
      new PublicKey('CawYq8Rmj4JRR992wVnGEFUjMEkmtmcFgEL4iS1qPczu'),
    ),
  ).toBe(true);
  programAddress = await PublicKey.createProgramAddress(
    ['Talking', 'Squirrels'],
    programId,
  );
  const programAddress2 = await PublicKey.createProgramAddress(
    ['Talking'],
    programId,
  );
  expect(programAddress.equals(programAddress2)).toBe(false);
});
