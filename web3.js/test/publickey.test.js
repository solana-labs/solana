// @flow
import {PublicKey} from '../src/publickey';

test('invalid', () => {
  expect(() => {
    new PublicKey([
      3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]);
  }).toThrow();

  expect(() => {
    new PublicKey('0x300000000000000000000000000000000000000000000000000000000000000000000');
  }).toThrow();

  expect(() => {
    new PublicKey('135693854574979916511997248057056142015550763280047535983739356259273198796800000');
  }).toThrow();
});

test('equals', () => {
  const arrayKey = new PublicKey([
    3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
  ]);
  const hexKey = new PublicKey('0x300000000000000000000000000000000000000000000000000000000000000');
  const decimalKey = new PublicKey('1356938545749799165119972480570561420155507632800475359837393562592731987968');
  const base56Key = new PublicKey('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  expect(arrayKey.equals(hexKey)).toBe(true);
  expect(arrayKey.equals(decimalKey)).toBe(true);
  expect(arrayKey.equals(base56Key)).toBe(true);

});

test('isPublicKey', () => {
  const key = new PublicKey('0x100000000000000000000000000000000000000000000000000000000000000');
  expect(PublicKey.isPublicKey(key)).toBe(true);
  expect(PublicKey.isPublicKey({})).toBe(false);
});

test('toBase58', () => {
  const key = new PublicKey('0x300000000000000000000000000000000000000000000000000000000000000');
  expect(key.toBase58()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');
  expect(key.toString()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  const key2 = new PublicKey('123456789');
  expect(key2.toBase58()).toBe('1111111111111111111111111111BukQL');
  expect(key2.toString()).toBe('1111111111111111111111111111BukQL');
});

test('toBuffer', () => {
  const key = new PublicKey('0x300000000000000000000000000000000000000000000000000000000000000');
  expect(key.toBuffer()).toHaveLength(32);
  expect(key.toBase58()).toBe('CiDwVBFgWV9E5MvXWoLgnEgn2hK7rJikbvfWavzAQz3');

  const key2 = new PublicKey('0x000000000000000000000000000000000000000000000000000000000000000');
  expect(key2.toBuffer()).toHaveLength(32);
  expect(key2.toBase58()).toBe('11111111111111111111111111111111');
});

test('equals (II)', () => {
  const key1 = new PublicKey([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
  ]);
  const key2 = new PublicKey(key1.toBuffer());

  expect(key1.equals(key2)).toBe(true);
});

