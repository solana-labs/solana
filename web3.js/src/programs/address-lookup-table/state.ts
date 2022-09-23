import * as BufferLayout from '@solana/buffer-layout';

import assert from '../../utils/assert';
import * as Layout from '../../layout';
import {PublicKey} from '../../publickey';
import {u64} from '../../utils/bigint';
import {decodeData} from '../../account-data';

export type AddressLookupTableState = {
  deactivationSlot: bigint;
  lastExtendedSlot: number;
  lastExtendedSlotStartIndex: number;
  authority?: PublicKey;
  addresses: Array<PublicKey>;
};

export type AddressLookupTableAccountArgs = {
  key: PublicKey;
  state: AddressLookupTableState;
};

/// The serialized size of lookup table metadata
const LOOKUP_TABLE_META_SIZE = 56;

export class AddressLookupTableAccount {
  key: PublicKey;
  state: AddressLookupTableState;

  constructor(args: AddressLookupTableAccountArgs) {
    this.key = args.key;
    this.state = args.state;
  }

  isActive(): boolean {
    const U64_MAX = BigInt('0xffffffffffffffff');
    return this.state.deactivationSlot === U64_MAX;
  }

  static deserialize(accountData: Uint8Array): AddressLookupTableState {
    const meta = decodeData(LookupTableMetaLayout, accountData);

    const serializedAddressesLen = accountData.length - LOOKUP_TABLE_META_SIZE;
    assert(serializedAddressesLen >= 0, 'lookup table is invalid');
    assert(serializedAddressesLen % 32 === 0, 'lookup table is invalid');

    const numSerializedAddresses = serializedAddressesLen / 32;
    const {addresses} = BufferLayout.struct<{addresses: Array<Uint8Array>}>([
      BufferLayout.seq(Layout.publicKey(), numSerializedAddresses, 'addresses'),
    ]).decode(accountData.slice(LOOKUP_TABLE_META_SIZE));

    return {
      deactivationSlot: meta.deactivationSlot,
      lastExtendedSlot: meta.lastExtendedSlot,
      lastExtendedSlotStartIndex: meta.lastExtendedStartIndex,
      authority:
        meta.authority.length !== 0
          ? new PublicKey(meta.authority[0])
          : undefined,
      addresses: addresses.map(address => new PublicKey(address)),
    };
  }
}

const LookupTableMetaLayout = {
  index: 1,
  layout: BufferLayout.struct<{
    typeIndex: number;
    deactivationSlot: bigint;
    lastExtendedSlot: number;
    lastExtendedStartIndex: number;
    authority: Array<Uint8Array>;
  }>([
    BufferLayout.u32('typeIndex'),
    u64('deactivationSlot'),
    BufferLayout.nu64('lastExtendedSlot'),
    BufferLayout.u8('lastExtendedStartIndex'),
    BufferLayout.u8(), // option
    BufferLayout.seq(
      Layout.publicKey(),
      BufferLayout.offset(BufferLayout.u8(), -1),
      'authority',
    ),
  ]),
};
