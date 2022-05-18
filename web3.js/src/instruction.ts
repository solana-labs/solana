import {Buffer} from 'buffer';
import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from './layout';

export interface IInstructionInputData {
  readonly instruction: number;
}

/**
 * @internal
 */
export type InstructionType<TInputData extends IInstructionInputData> = {
  /** The Instruction index (from solana upstream program) */
  index: number;
  /** The BufferLayout to use to build data */
  layout: BufferLayout.Layout<TInputData>;
};

/**
 * Populate a buffer of instruction data using an InstructionType
 * @internal
 */
export function encodeData<TInputData extends IInstructionInputData>(
  type: InstructionType<TInputData>,
  fields?: any,
): Buffer {
  const allocLength =
    type.layout.span >= 0 ? type.layout.span : Layout.getAlloc(type, fields);
  const data = Buffer.alloc(allocLength);
  const layoutFields = Object.assign({instruction: type.index}, fields);
  type.layout.encode(layoutFields, data);
  return data;
}

/**
 * Decode instruction data buffer using an InstructionType
 * @internal
 */
export function decodeData<TInputData extends IInstructionInputData>(
  type: InstructionType<TInputData>,
  buffer: Buffer,
): TInputData {
  let data: TInputData;
  try {
    data = type.layout.decode(buffer);
  } catch (err) {
    throw new Error('invalid instruction; ' + err);
  }

  if (data.instruction !== type.index) {
    throw new Error(
      `invalid instruction; instruction index mismatch ${data.instruction} != ${type.index}`,
    );
  }

  return data;
}
