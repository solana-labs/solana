// @flow

import {expect} from 'chai';
import {decodeLength, encodeLength} from '../src/util/shortvec-encoding';

function checkDecodedArray(array: Array<number>, expectedValue: number) {
  expect(decodeLength(array)).to.eq(expectedValue);
  expect(array).to.have.length(0);
}

function checkEncodedArray(
  array: Array<number>,
  len: number,
  prevLength: number,
  addedLength: number,
  expectedArray: Array<number>,
) {
  encodeLength(array, len);
  expect(array).to.have.length(prevLength);
  expect(array.slice(-addedLength)).to.eql(expectedArray);
}

describe('shortvec', () => {
  it('decodeLength', () => {
    let array = [];
    checkDecodedArray(array, 0);

    array = [5];
    checkDecodedArray(array, 5);

    array = [0x7f];
    checkDecodedArray(array, 0x7f);

    array = [0x80, 0x01];
    checkDecodedArray(array, 0x80);

    array = [0xff, 0x01];
    checkDecodedArray(array, 0xff);

    array = [0x80, 0x02];
    checkDecodedArray(array, 0x100);

    array = [0xff, 0xff, 0x01];
    checkDecodedArray(array, 0x7fff);

    array = [0x80, 0x80, 0x80, 0x01];
    checkDecodedArray(array, 0x200000);
  });

  it('encodeLength', () => {
    let array = [];
    let prevLength = 1;
    checkEncodedArray(array, 0, prevLength, 1, [0]);

    checkEncodedArray(array, 5, (prevLength += 1), 1, [5]);

    checkEncodedArray(array, 0x7f, (prevLength += 1), 1, [0x7f]);

    checkEncodedArray(array, 0x80, (prevLength += 2), 2, [0x80, 0x01]);

    checkEncodedArray(array, 0xff, (prevLength += 2), 2, [0xff, 0x01]);

    checkEncodedArray(array, 0x100, (prevLength += 2), 2, [0x80, 0x02]);

    checkEncodedArray(array, 0x7fff, (prevLength += 3), 3, [0xff, 0xff, 0x01]);

    prevLength = checkEncodedArray(array, 0x200000, (prevLength += 4), 4, [
      0x80,
      0x80,
      0x80,
      0x01,
    ]);
  });
});
