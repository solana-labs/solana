/* eslint-disable @typescript-eslint/no-redeclare */

import { decodeInstruction, MARKETS } from "@project-serum/serum";
import {
  AccountMeta,
  PublicKey,
  SignatureResult,
  TransactionInstruction,
} from "@solana/web3.js";
import { enums, number, type, Infer, create } from "superstruct";
import { BigIntFromString } from "validators/number";

export const OPEN_BOOK_PROGRAM_ID =
  "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX";

const SERUM_PROGRAM_IDS = [
  "4ckmDgGdxQoPDLUkDT3vHgSAkzA3QRdNq5ywwY4sUSJn",
  "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin",
  OPEN_BOOK_PROGRAM_ID,
];

export const SERUM_DECODED_MAX = 6;

export type Side = Infer<typeof Side>;
export const Side = enums(["buy", "sell"]);

export type OrderType = Infer<typeof OrderType>;
export const OrderType = enums(["limit", "ioc", "postOnly"]);

export type SelfTradeBehavior = Infer<typeof SelfTradeBehavior>;
export const SelfTradeBehavior = enums([
  "decrementTake",
  "cancelProvide",
  "abortTransaction",
]);

function getOptionalKey(
  keys: AccountMeta[],
  index: number
): PublicKey | undefined {
  if (index < keys.length) {
    return keys[index].pubkey;
  } else {
    return undefined;
  }
}

export type InitializeMarket = {
  programId: PublicKey;
  data: Infer<typeof InitializeMarketInstruction>;
  accounts: {
    market: PublicKey;
    requestQueue: PublicKey;
    eventQueue: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
    baseVault: PublicKey;
    quoteVault: PublicKey;
    baseMint: PublicKey;
    quoteMint: PublicKey;
    openOrdersMarketAuthority?: PublicKey;
    pruneAuthority?: PublicKey;
    crankAuthority?: PublicKey;
  };
};

export const InitializeMarketInstruction = type({
  baseLotSize: BigIntFromString,
  quoteLotSize: BigIntFromString,
  feeRateBps: number(),
  quoteDustThreshold: BigIntFromString,
  vaultSignerNonce: BigIntFromString,
});

export function decodeInitializeMarket(
  ix: TransactionInstruction
): InitializeMarket {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).initializeMarket,
      InitializeMarketInstruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      requestQueue: ix.keys[1].pubkey,
      eventQueue: ix.keys[2].pubkey,
      bids: ix.keys[3].pubkey,
      asks: ix.keys[4].pubkey,
      baseVault: ix.keys[5].pubkey,
      quoteVault: ix.keys[6].pubkey,
      baseMint: ix.keys[7].pubkey,
      quoteMint: ix.keys[8].pubkey,
      openOrdersMarketAuthority: getOptionalKey(ix.keys, 10),
      pruneAuthority: getOptionalKey(ix.keys, 11),
      crankAuthority: getOptionalKey(ix.keys, 12),
    },
  };
}

export type NewOrder = {
  programId: PublicKey;
  data: Infer<typeof NewOrderInstruction>;
  accounts: {
    market: PublicKey;
    openOrders: PublicKey;
    requestQueue: PublicKey;
    payer: PublicKey;
    openOrdersOwner: PublicKey;
    baseVault: PublicKey;
    quoteVault: PublicKey;
    feeDiscountPubkey?: PublicKey;
  };
};

export const NewOrderInstruction = type({
  side: Side,
  limitPrice: BigIntFromString,
  maxQuantity: BigIntFromString,
  orderType: OrderType,
  clientId: BigIntFromString,
});

export function decodeNewOrder(ix: TransactionInstruction): NewOrder {
  return {
    programId: ix.programId,
    data: create(decodeInstruction(ix.data).newOrder, NewOrderInstruction),
    accounts: {
      market: ix.keys[0].pubkey,
      openOrders: ix.keys[1].pubkey,
      requestQueue: ix.keys[2].pubkey,
      payer: ix.keys[3].pubkey,
      openOrdersOwner: ix.keys[4].pubkey,
      baseVault: ix.keys[5].pubkey,
      quoteVault: ix.keys[6].pubkey,
      feeDiscountPubkey: getOptionalKey(ix.keys, 9),
    },
  };
}

export type MatchOrders = {
  programId: PublicKey;
  data: Infer<typeof MatchOrdersInstruction>;
  accounts: {
    market: PublicKey;
    requestQueue: PublicKey;
    eventQueue: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
  };
};

export const MatchOrdersInstruction = type({
  limit: number(),
});

export function decodeMatchOrders(ix: TransactionInstruction): MatchOrders {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).matchOrders,
      MatchOrdersInstruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      requestQueue: ix.keys[1].pubkey,
      eventQueue: ix.keys[2].pubkey,
      bids: ix.keys[3].pubkey,
      asks: ix.keys[4].pubkey,
    },
  };
}

export type ConsumeEvents = {
  programId: PublicKey;
  data: Infer<typeof ConsumeEventsInstruction>;
  accounts: {
    openOrders: PublicKey[];
    market: PublicKey;
    eventQueue: PublicKey;
  };
};

export const ConsumeEventsInstruction = type({
  limit: number(),
});

export function decodeConsumeEvents(ix: TransactionInstruction): ConsumeEvents {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).consumeEvents,
      ConsumeEventsInstruction
    ),
    accounts: {
      openOrders: ix.keys.slice(0, -4).map((k) => k.pubkey),
      market: ix.keys[ix.keys.length - 4].pubkey,
      eventQueue: ix.keys[ix.keys.length - 3].pubkey,
    },
  };
}

export type CancelOrder = {
  programId: PublicKey;
  data: Infer<typeof CancelOrderInstruction>;
  accounts: {
    market: PublicKey;
    openOrders: PublicKey;
    requestQueue: PublicKey;
    openOrdersOwner: PublicKey;
  };
};

export const CancelOrderInstruction = type({
  side: Side,
  orderId: BigIntFromString,
  openOrdersSlot: number(),
});

export function decodeCancelOrder(ix: TransactionInstruction): CancelOrder {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).cancelOrder,
      CancelOrderInstruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      openOrders: ix.keys[1].pubkey,
      requestQueue: ix.keys[2].pubkey,
      openOrdersOwner: ix.keys[3].pubkey,
    },
  };
}

export type SettleFunds = {
  programId: PublicKey;
  accounts: {
    market: PublicKey;
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    baseVault: PublicKey;
    quoteVault: PublicKey;
    baseWallet: PublicKey;
    quoteWallet: PublicKey;
    vaultSigner: PublicKey;
    referrerQuoteWallet?: PublicKey;
  };
};

export function decodeSettleFunds(ix: TransactionInstruction): SettleFunds {
  return {
    programId: ix.programId,
    accounts: {
      market: ix.keys[0].pubkey,
      openOrders: ix.keys[1].pubkey,
      openOrdersOwner: ix.keys[2].pubkey,
      baseVault: ix.keys[3].pubkey,
      quoteVault: ix.keys[4].pubkey,
      baseWallet: ix.keys[5].pubkey,
      quoteWallet: ix.keys[6].pubkey,
      vaultSigner: ix.keys[7].pubkey,
      referrerQuoteWallet: getOptionalKey(ix.keys, 9),
    },
  };
}

export type CancelOrderByClientId = {
  programId: PublicKey;
  data: Infer<typeof CancelOrderByClientIdInstruction>;
  accounts: {
    market: PublicKey;
    openOrders: PublicKey;
    requestQueue: PublicKey;
    openOrdersOwner: PublicKey;
  };
};

export const CancelOrderByClientIdInstruction = type({
  clientId: BigIntFromString,
});

export function decodeCancelOrderByClientId(
  ix: TransactionInstruction
): CancelOrderByClientId {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).cancelOrderByClientId,
      CancelOrderByClientIdInstruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      openOrders: ix.keys[1].pubkey,
      requestQueue: ix.keys[2].pubkey,
      openOrdersOwner: ix.keys[3].pubkey,
    },
  };
}

export type DisableMarket = {
  programId: PublicKey;
  accounts: {
    market: PublicKey;
    disableAuthority: PublicKey;
  };
};

export function decodeDisableMarket(ix: TransactionInstruction): DisableMarket {
  return {
    programId: ix.programId,
    accounts: {
      market: ix.keys[0].pubkey,
      disableAuthority: ix.keys[1].pubkey,
    },
  };
}

export type SweepFees = {
  programId: PublicKey;
  accounts: {
    market: PublicKey;
    quoteVault: PublicKey;
    feeSweepingAuthority: PublicKey;
    quoteFeeReceiver: PublicKey;
    vaultSigner: PublicKey;
  };
};

export function decodeSweepFees(ix: TransactionInstruction): SweepFees {
  return {
    programId: ix.programId,
    accounts: {
      market: ix.keys[0].pubkey,
      quoteVault: ix.keys[1].pubkey,
      feeSweepingAuthority: ix.keys[2].pubkey,
      quoteFeeReceiver: ix.keys[3].pubkey,
      vaultSigner: ix.keys[4].pubkey,
    },
  };
}

export type NewOrderV3 = {
  programId: PublicKey;
  data: Infer<typeof NewOrderV3Instruction>;
  accounts: {
    market: PublicKey;
    openOrders: PublicKey;
    requestQueue: PublicKey;
    eventQueue: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
    payer: PublicKey;
    openOrdersOwner: PublicKey;
    baseVault: PublicKey;
    quoteVault: PublicKey;
    feeDiscountPubkey?: PublicKey;
  };
};

export const NewOrderV3Instruction = type({
  side: Side,
  limitPrice: BigIntFromString,
  maxBaseQuantity: BigIntFromString,
  maxQuoteQuantity: BigIntFromString,
  selfTradeBehavior: SelfTradeBehavior,
  orderType: OrderType,
  clientId: BigIntFromString,
  limit: number(),
});

export function decodeNewOrderV3(ix: TransactionInstruction): NewOrderV3 {
  return {
    programId: ix.programId,
    data: create(decodeInstruction(ix.data).newOrderV3, NewOrderV3Instruction),
    accounts: {
      market: ix.keys[0].pubkey,
      openOrders: ix.keys[1].pubkey,
      requestQueue: ix.keys[2].pubkey,
      eventQueue: ix.keys[3].pubkey,
      bids: ix.keys[4].pubkey,
      asks: ix.keys[5].pubkey,
      payer: ix.keys[6].pubkey,
      openOrdersOwner: ix.keys[7].pubkey,
      baseVault: ix.keys[8].pubkey,
      quoteVault: ix.keys[9].pubkey,
      feeDiscountPubkey: getOptionalKey(ix.keys, 12),
    },
  };
}

export type CancelOrderV2 = {
  programId: PublicKey;
  data: Infer<typeof CancelOrderV2Instruction>;
  accounts: {
    market: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    eventQueue: PublicKey;
  };
};

export const CancelOrderV2Instruction = type({
  side: Side,
  orderId: BigIntFromString,
});

export function decodeCancelOrderV2(ix: TransactionInstruction): CancelOrderV2 {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).cancelOrderV2,
      CancelOrderV2Instruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      bids: ix.keys[1].pubkey,
      asks: ix.keys[2].pubkey,
      openOrders: ix.keys[3].pubkey,
      openOrdersOwner: ix.keys[4].pubkey,
      eventQueue: ix.keys[5].pubkey,
    },
  };
}

export type CancelOrderByClientIdV2 = {
  programId: PublicKey;
  data: Infer<typeof CancelOrderByClientIdV2Instruction>;
  accounts: {
    market: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    eventQueue: PublicKey;
  };
};

export const CancelOrderByClientIdV2Instruction = type({
  clientId: BigIntFromString,
});

export function decodeCancelOrderByClientIdV2(
  ix: TransactionInstruction
): CancelOrderByClientIdV2 {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).cancelOrderByClientIdV2,
      CancelOrderByClientIdV2Instruction
    ),
    accounts: {
      market: ix.keys[0].pubkey,
      bids: ix.keys[1].pubkey,
      asks: ix.keys[2].pubkey,
      openOrders: ix.keys[3].pubkey,
      openOrdersOwner: ix.keys[4].pubkey,
      eventQueue: ix.keys[5].pubkey,
    },
  };
}

export type CloseOpenOrders = {
  programId: PublicKey;
  accounts: {
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    rentReceiver: PublicKey;
    market: PublicKey;
  };
};

export function decodeCloseOpenOrders(
  ix: TransactionInstruction
): CloseOpenOrders {
  return {
    programId: ix.programId,
    accounts: {
      openOrders: ix.keys[0].pubkey,
      openOrdersOwner: ix.keys[1].pubkey,
      rentReceiver: ix.keys[2].pubkey,
      market: ix.keys[3].pubkey,
    },
  };
}

export type InitOpenOrders = {
  programId: PublicKey;
  accounts: {
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    market: PublicKey;
    openOrdersMarketAuthority?: PublicKey;
  };
};

export function decodeInitOpenOrders(
  ix: TransactionInstruction
): InitOpenOrders {
  return {
    programId: ix.programId,
    accounts: {
      openOrders: ix.keys[0].pubkey,
      openOrdersOwner: ix.keys[1].pubkey,
      market: ix.keys[2].pubkey,
      openOrdersMarketAuthority: ix.keys[4]?.pubkey,
    },
  };
}

export type Prune = {
  programId: PublicKey;
  data: Infer<typeof PruneInstruction>;
  accounts: {
    market: PublicKey;
    bids: PublicKey;
    asks: PublicKey;
    pruneAuthority: PublicKey;
    openOrders: PublicKey;
    openOrdersOwner: PublicKey;
    eventQueue: PublicKey;
  };
};

export const PruneInstruction = type({
  limit: number(),
});

export function decodePrune(ix: TransactionInstruction): Prune {
  return {
    programId: ix.programId,
    data: create(decodeInstruction(ix.data).prune, PruneInstruction),
    accounts: {
      market: ix.keys[0].pubkey,
      bids: ix.keys[1].pubkey,
      asks: ix.keys[2].pubkey,
      pruneAuthority: ix.keys[3].pubkey,
      openOrders: ix.keys[4].pubkey,
      openOrdersOwner: ix.keys[5].pubkey,
      eventQueue: ix.keys[6].pubkey,
    },
  };
}

export type ConsumeEventsPermissioned = {
  programId: PublicKey;
  data: Infer<typeof ConsumeEventsPermissionedInstruction>;
  accounts: {
    openOrders: PublicKey[];
    market: PublicKey;
    eventQueue: PublicKey;
    crankAuthority: PublicKey;
  };
};

export const ConsumeEventsPermissionedInstruction = type({
  limit: number(),
});

export function decodeConsumeEventsPermissioned(
  ix: TransactionInstruction
): ConsumeEventsPermissioned {
  return {
    programId: ix.programId,
    data: create(
      decodeInstruction(ix.data).consumeEventsPermissioned,
      ConsumeEventsPermissionedInstruction
    ),
    accounts: {
      openOrders: ix.keys.slice(0, -3).map((k) => k.pubkey),
      market: ix.keys[ix.keys.length - 3].pubkey,
      eventQueue: ix.keys[ix.keys.length - 2].pubkey,
      crankAuthority: ix.keys[ix.keys.length - 1].pubkey,
    },
  };
}

export function isSerumInstruction(instruction: TransactionInstruction) {
  return (
    SERUM_PROGRAM_IDS.includes(instruction.programId.toBase58()) ||
    MARKETS.some(
      (market) =>
        market.programId && market.programId.equals(instruction.programId)
    )
  );
}

export function parseSerumInstructionKey(
  instruction: TransactionInstruction
): string {
  const decoded = decodeInstruction(instruction.data);
  const keys = Object.keys(decoded);

  if (keys.length < 1) {
    throw new Error("Serum instruction key not decoded");
  }

  return keys[0];
}

const SERUM_CODE_LOOKUP: { [key: number]: string } = {
  0: "Initialize Market",
  1: "New Order",
  2: "Match Orders",
  3: "Consume Events",
  4: "Cancel Order",
  5: "Settle Funds",
  6: "Cancel Order by Client Id",
  7: "Disable Market",
  8: "Sweep Fees",
  9: "New Order v2",
  10: "New Order v3",
  11: "Cancel Order v2",
  12: "Cancel Order by Client Id v2",
  13: "Send Take",
  14: "Close Open Orders",
  15: "Init Open Orders",
  16: "Prune",
  17: "Consume Events Permissioned",
};

export function parseSerumInstructionCode(instruction: TransactionInstruction) {
  return instruction.data.slice(1, 5).readUInt32LE(0);
}

export function parseSerumInstructionTitle(
  instruction: TransactionInstruction
): string {
  const code = parseSerumInstructionCode(instruction);

  if (!(code in SERUM_CODE_LOOKUP)) {
    throw new Error(`Unrecognized Serum instruction code: ${code}`);
  }

  return SERUM_CODE_LOOKUP[code];
}

export type SerumIxDetailsProps<T> = {
  ix: TransactionInstruction;
  index: number;
  result: SignatureResult;
  info: T;
  programName: string;
  innerCards?: JSX.Element[];
  childIndex?: number;
};
