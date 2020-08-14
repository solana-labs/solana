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
      '0x300000000000000000000000000000000000000000000000000000000000000',
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
  const base58Key = new PublicKey(
    'CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3',
  );

  expect(arrayKey.equals(base58Key)).toBe(true);
});

test('toBase58', () => {
  const key = new PublicKey('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
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
  const key = new PublicKey('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
  expect(key.toBuffer()).toHaveLength(32);
  expect(key.toBase58()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  const key2 = new PublicKey('11111111111111111111111111111111');
  expect(key2.toBuffer()).toHaveLength(32);
  expect(key2.toBase58()).toBe('11111111111111111111111111111111');

  const key3 = new PublicKey(0);
  expect(key3.toBuffer()).toHaveLength(32);
  expect(key3.toBase58()).toBe('11111111111111111111111111111111');
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
  const publicKey = new PublicKey(
    'SeedPubey1111111111111111111111111111111111',
  );

  let programAddress = await PublicKey.createProgramAddress(
    [Buffer.from('', 'utf8'), Buffer.from([1])],
    programId,
  );
  expect(
    programAddress.equals(
      new PublicKey('3gF2KMe9KiC6FNVBmfg9i267aMPvK37FewCip4eGBFcT'),
    ),
  ).toBe(true);

  programAddress = await PublicKey.createProgramAddress(
    [Buffer.from('☉', 'utf8')],
    programId,
  );
  expect(
    programAddress.equals(
      new PublicKey('7ytmC1nT1xY4RfxCV2ZgyA7UakC93do5ZdyhdF3EtPj7'),
    ),
  ).toBe(true);

  programAddress = await PublicKey.createProgramAddress(
    [Buffer.from('Talking', 'utf8'), Buffer.from('Squirrels', 'utf8')],
    programId,
  );
  expect(
    programAddress.equals(
      new PublicKey('HwRVBufQ4haG5XSgpspwKtNd3PC9GM9m1196uJW36vds'),
    ),
  ).toBe(true);

  programAddress = await PublicKey.createProgramAddress(
    [publicKey.toBuffer()],
    programId,
  );
  expect(
    programAddress.equals(
      new PublicKey('GUs5qLUfsEHkcMB9T38vjr18ypEhRuNWiePW2LoK4E3K'),
    ),
  ).toBe(true);

  const programAddress2 = await PublicKey.createProgramAddress(
    [Buffer.from('Talking', 'utf8')],
    programId,
  );
  expect(programAddress.equals(programAddress2)).toBe(false);
});

test('findProgramAddress', async () => {
  const programId = new PublicKey(
    'BPFLoader1111111111111111111111111111111111',
  );
  let [programAddress, nonce] = await PublicKey.findProgramAddress(
    [Buffer.from('', 'utf8')],
    programId,
  );
  expect(
    programAddress.equals(
      await PublicKey.createProgramAddress(
        [Buffer.from('', 'utf8'), Buffer.from([nonce])],
        programId,
      ),
    ),
  ).toBe(true);
});
