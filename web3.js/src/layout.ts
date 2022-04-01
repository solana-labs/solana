import {Buffer} from 'buffer';
import * as BufferLayout from '@solana/buffer-layout';

/**
 * Layout for a public key
 */
export const publicKey = (property: string = 'publicKey') => {
  return BufferLayout.blob(32, property);
};

/**
 * Layout for a 64bit unsigned value
 */
export const uint64 = (property: string = 'uint64') => {
  return BufferLayout.blob(8, property);
};

interface IRustStringShim
  extends Omit<
    BufferLayout.Structure<
      Readonly<{
        length: number;
        lengthPadding: number;
        chars: Uint8Array;
      }>
    >,
    'decode' | 'encode' | 'replicate'
  > {
  alloc: (str: string) => number;
  decode: (b: Uint8Array, offset?: number) => string;
  encode: (str: string, b: Uint8Array, offset?: number) => number;
  replicate: (property: string) => this;
}

/**
 * Layout for a Rust String type
 */
export const rustString = (
  property: string = 'string',
): BufferLayout.Layout<string> => {
  const rsl = BufferLayout.struct<
    Readonly<{
      length?: number;
      lengthPadding?: number;
      chars: Uint8Array;
    }>
  >(
    [
      BufferLayout.u32('length'),
      BufferLayout.u32('lengthPadding'),
      BufferLayout.blob(BufferLayout.offset(BufferLayout.u32(), -8), 'chars'),
    ],
    property,
  );
  const _decode = rsl.decode.bind(rsl);
  const _encode = rsl.encode.bind(rsl);

  const rslShim = rsl as unknown as IRustStringShim;

  rslShim.decode = (b: Uint8Array, offset?: number) => {
    const data = _decode(b, offset);
    return data['chars'].toString();
  };

  rslShim.encode = (str: string, b: Uint8Array, offset?: number) => {
    const data = {
      chars: Buffer.from(str, 'utf8'),
    };
    return _encode(data, b, offset);
  };

  rslShim.alloc = (str: string) => {
    return (
      BufferLayout.u32().span +
      BufferLayout.u32().span +
      Buffer.from(str, 'utf8').length
    );
  };

  return rslShim;
};

/**
 * Layout for an Authorized object
 */
export const authorized = (property: string = 'authorized') => {
  return BufferLayout.struct<
    Readonly<{
      staker: Uint8Array;
      withdrawer: Uint8Array;
    }>
  >([publicKey('staker'), publicKey('withdrawer')], property);
};

/**
 * Layout for a Lockup object
 */
export const lockup = (property: string = 'lockup') => {
  return BufferLayout.struct<
    Readonly<{
      custodian: Uint8Array;
      epoch: number;
      unixTimestamp: number;
    }>
  >(
    [
      BufferLayout.ns64('unixTimestamp'),
      BufferLayout.ns64('epoch'),
      publicKey('custodian'),
    ],
    property,
  );
};

/**
 *  Layout for a VoteInit object
 */
export const voteInit = (property: string = 'voteInit') => {
  return BufferLayout.struct<
    Readonly<{
      authorizedVoter: Uint8Array;
      authorizedWithdrawer: Uint8Array;
      commission: number;
      nodePubkey: Uint8Array;
    }>
  >(
    [
      publicKey('nodePubkey'),
      publicKey('authorizedVoter'),
      publicKey('authorizedWithdrawer'),
      BufferLayout.u8('commission'),
    ],
    property,
  );
};

export function getAlloc(type: any, fields: any): number {
  let alloc = 0;
  type.layout.fields.forEach((item: any) => {
    if (item.span >= 0) {
      alloc += item.span;
    } else if (typeof item.alloc === 'function') {
      alloc += item.alloc(fields[item.property]);
    }
  });
  return alloc;
}
