/* eslint-disable @typescript-eslint/no-redeclare */

import { Infer, number, enums, type, array, optional } from "superstruct";
import { PublicKeyFromString } from "validators/pubkey";
import { BigIntFromString, NumberFromString } from "validators/number";

export type AddressLookupTableAccountType = Infer<
  typeof AddressLookupTableAccountType
>;
export const AddressLookupTableAccountType = enums([
  "uninitialized",
  "lookupTable",
]);

export type AddressLookupTableAccountInfo = Infer<
  typeof AddressLookupTableAccountInfo
>;
export const AddressLookupTableAccountInfo = type({
  deactivationSlot: BigIntFromString,
  lastExtendedSlot: NumberFromString,
  lastExtendedSlotStartIndex: number(),
  authority: optional(PublicKeyFromString),
  addresses: array(PublicKeyFromString),
});

export type ParsedAddressLookupTableAccount = Infer<
  typeof ParsedAddressLookupTableAccount
>;
export const ParsedAddressLookupTableAccount = type({
  type: AddressLookupTableAccountType,
  info: AddressLookupTableAccountInfo,
});
