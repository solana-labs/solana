import {Buffer} from 'buffer';
import {blob, Layout} from '@solana/buffer-layout';
import {toBigIntLE, toBufferLE} from 'bigint-buffer';

interface EncodeDecode<T> {
  decode(buffer: Buffer, offset?: number): T;
  encode(src: T, buffer: Buffer, offset?: number): number;
}

const encodeDecode = <T>(layout: Layout<T>): EncodeDecode<T> => {
  const decode = layout.decode.bind(layout);
  const encode = layout.encode.bind(layout);
  return {decode, encode};
};

const bigInt =
  (length: number) =>
  (property?: string): Layout<bigint> => {
    const layout = blob(length, property);
    const {encode, decode} = encodeDecode(layout);

    const bigIntLayout = layout as Layout<unknown> as Layout<bigint>;

    bigIntLayout.decode = (buffer: Buffer, offset: number) => {
      const src = decode(buffer, offset);
      return toBigIntLE(Buffer.from(src));
    };

    bigIntLayout.encode = (bigInt: bigint, buffer: Buffer, offset: number) => {
      const src = toBufferLE(bigInt, length);
      return encode(src, buffer, offset);
    };

    return bigIntLayout;
  };

export const u64 = bigInt(8);

export const u128 = bigInt(16);

export const u192 = bigInt(24);

export const u256 = bigInt(32);
