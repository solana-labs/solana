/* eslint-disable @typescript-eslint/no-redeclare */
import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import {
  enums,
  number,
  type,
  Infer,
  create,
  array,
  string,
  optional,
  coerce,
  any,
} from "superstruct";
import {
  Instruction,
  decodeInstruction,
  BONFIDABOT_PROGRAM_ID,
} from "@bonfida/bot";

export const SERUM_DECODED_MAX = 6;

export type Side = Infer<typeof Side>;
export const Side = enums([0, 1]);

export type OrderType = Infer<typeof OrderType>;
export const OrderType = enums([0, 1, 2]);

const PublicKeyToString = coerce(string(), any(), (value) => value.toBase58());

export type InitializeBot = {
  systemProgramAccount: PublicKey;
  rentSysvarAccount: PublicKey;
  splTokenProgramAccount: PublicKey;
  poolAccount: PublicKey;
  mintAccount: PublicKey;
  payerAccount: PublicKey;

  poolSeed: string;
  maxNumberOfAsset: number | undefined | null;
  numberOfMarkets: number;
  programId: PublicKey;
};

export const InitializeBotDecode = type({
  poolSeed: string(),
  maxNumberOfAsset: optional(number()),
  numberOfMarkets: number(),
});

export const decodeInitializeBot = (
  ix: TransactionInstruction
): InitializeBot => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.Init),
    InitializeBotDecode
  );
  const initializeBot: InitializeBot = {
    systemProgramAccount: ix.keys[0].pubkey,
    rentSysvarAccount: ix.keys[1].pubkey,
    splTokenProgramAccount: ix.keys[2].pubkey,
    poolAccount: ix.keys[3].pubkey,
    mintAccount: ix.keys[4].pubkey,
    payerAccount: ix.keys[5].pubkey,
    poolSeed: decoded.poolSeed,
    maxNumberOfAsset: decoded.maxNumberOfAsset,
    numberOfMarkets: decoded.numberOfMarkets,
    programId: ix.programId,
  };

  return initializeBot;
};

export type CreateBot = {
  splTokenProgramId: PublicKey;
  clockSysvarKey: PublicKey;
  mintKey: PublicKey;
  poolKey: PublicKey;
  poolSeed: string;
  targetPoolTokenKey: PublicKey;
  serumProgramId: PublicKey;
  signalProviderKey: PublicKey;
  depositAmounts: number[] | undefined | null;
  markets: string[] | undefined | null;
  feeCollectionPeriod: number;
  feeRatio: number;
  programId: PublicKey;
};

export const CreateBotDecode = type({
  poolSeed: string(),
  feeCollectionPeriod: number(),
  feeRatio: number(),
  depositAmounts: array(number()),
  markets: array(PublicKeyToString),
});

export const decodeCreateBot = (ix: TransactionInstruction): CreateBot => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.Create),
    CreateBotDecode
  );
  const createBot: CreateBot = {
    splTokenProgramId: ix.keys[0].pubkey,
    clockSysvarKey: ix.keys[1].pubkey,
    mintKey: ix.keys[4].pubkey,
    poolKey: ix.keys[6].pubkey,
    poolSeed: decoded.poolSeed,
    targetPoolTokenKey: ix.keys[5].pubkey,
    serumProgramId: ix.keys[2].pubkey,
    signalProviderKey: ix.keys[3].pubkey,
    depositAmounts: decoded.depositAmounts,
    markets: decoded.markets,
    feeCollectionPeriod: decoded.feeCollectionPeriod,
    feeRatio: decoded.feeRatio,
    programId: ix.programId,
  };

  return createBot;
};

export type Deposit = {
  splTokenProgramId: PublicKey;
  programId: PublicKey;
  sigProviderFeeReceiverKey: PublicKey;
  bonfidaFeeReceiverKey: PublicKey;
  bonfidaBuyAndBurnKey: PublicKey;
  mintKey: PublicKey;
  poolKey: PublicKey;
  targetPoolTokenKey: PublicKey;
  poolSeed: string;
  poolTokenAmount: number;
};

export const DepositDecode = type({
  poolSeed: string(),
  poolTokenAmount: number(),
});

export const decodeDeposit = (ix: TransactionInstruction): Deposit => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.Deposit),
    DepositDecode
  );

  const deposit: Deposit = {
    splTokenProgramId: ix.keys[0].pubkey,
    programId: ix.programId,
    sigProviderFeeReceiverKey: ix.keys[3].pubkey,
    bonfidaFeeReceiverKey: ix.keys[4].pubkey,
    bonfidaBuyAndBurnKey: ix.keys[5].pubkey,
    mintKey: ix.keys[1].pubkey,
    poolKey: ix.keys[6].pubkey,
    targetPoolTokenKey: ix.keys[2].pubkey,
    poolSeed: decoded.poolSeed,
    poolTokenAmount: decoded.poolTokenAmount,
  };

  return deposit;
};

export type CreateOrder = {
  programId: PublicKey;
  signalProviderKey: PublicKey;
  market: PublicKey;
  payerPoolAssetKey: PublicKey;
  openOrdersKey: PublicKey;
  serumRequestQueue: PublicKey;
  serumEventQueue: PublicKey;
  serumMarketBids: PublicKey;
  serumMarketAsks: PublicKey;
  poolKey: PublicKey;
  coinVaultKey: PublicKey;
  pcVaultKey: PublicKey;
  splTokenProgramId: PublicKey;
  dexProgramKey: PublicKey;
  rentProgramId: PublicKey;
  srmReferrerKey: PublicKey | null | undefined;
  poolSeed: string;
  side: Side;
  limitPrice: number;
  ratioOfPoolAssetsToTrade: number;
  orderType: OrderType;
  clientId: number;
  coinLotSize: number;
  pcLotSize: number;
  targetMint: string;
};

export const CreateDecode = type({
  poolSeed: string(),
  side: Side,
  limitPrice: number(),
  ratioOfPoolAssetsToTrade: number(),
  orderType: OrderType,
  clientId: number(),
  coinLotSize: number(),
  pcLotSize: number(),
  targetMint: string(),
});

export const decodeCreateOrder = (ix: TransactionInstruction): CreateOrder => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.CreateOrder),
    CreateDecode
  );

  const createOrder: CreateOrder = {
    programId: ix.programId,
    signalProviderKey: ix.keys[0].pubkey,
    market: ix.keys[1].pubkey,
    payerPoolAssetKey: ix.keys[2].pubkey,
    openOrdersKey: ix.keys[3].pubkey,
    serumRequestQueue: ix.keys[5].pubkey,
    serumEventQueue: ix.keys[4].pubkey,
    serumMarketBids: ix.keys[6].pubkey,
    serumMarketAsks: ix.keys[7].pubkey,
    poolKey: ix.keys[8].pubkey,
    coinVaultKey: ix.keys[9].pubkey,
    pcVaultKey: ix.keys[10].pubkey,
    splTokenProgramId: ix.keys[11].pubkey,
    dexProgramKey: ix.keys[13].pubkey,
    rentProgramId: ix.keys[12].pubkey,
    srmReferrerKey: ix.keys[14]?.pubkey,
    // Miss maxQuantity
    //
    poolSeed: decoded.poolSeed,
    side: decoded.side,
    limitPrice: decoded.limitPrice,
    ratioOfPoolAssetsToTrade: decoded.ratioOfPoolAssetsToTrade,
    orderType: decoded.orderType,
    clientId: decoded.clientId,
    coinLotSize: decoded.coinLotSize,
    pcLotSize: decoded.pcLotSize,
    targetMint: decoded.targetMint,
  };

  return createOrder;
};

export type CancelOrder = {
  programId: PublicKey;
  signalProviderKey: PublicKey;
  market: PublicKey;
  openOrdersKey: PublicKey;
  serumEventQueue: PublicKey;
  serumMarketBids: PublicKey;
  serumMarketAsks: PublicKey;
  poolKey: PublicKey;
  dexProgramKey: PublicKey;
  poolSeed: string;
  side: Side;
  orderId: number;
};

export const CancelOrderDecode = type({
  poolSeed: string(),
  side: Side,
  orderId: number(),
});

export const decodeCancelOrder = (ix: TransactionInstruction): CancelOrder => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.CancelOrder),
    CancelOrderDecode
  );

  const cancelOrder: CancelOrder = {
    programId: ix.programId,
    signalProviderKey: ix.keys[0].pubkey,
    market: ix.keys[1].pubkey,
    openOrdersKey: ix.keys[2].pubkey,
    serumEventQueue: ix.keys[3].pubkey,
    serumMarketBids: ix.keys[4].pubkey,
    serumMarketAsks: ix.keys[5].pubkey,
    poolKey: ix.keys[6].pubkey,
    dexProgramKey: ix.keys[7].pubkey,
    //
    poolSeed: decoded.poolSeed,
    side: decoded.side,
    orderId: decoded.orderId,
  };
  return cancelOrder;
};

export type SettleFunds = {
  programId: PublicKey;
  market: PublicKey;
  openOrdersKey: PublicKey;
  poolKey: PublicKey;
  poolMintKey: PublicKey;
  coinVaultKey: PublicKey;
  pcVaultKey: PublicKey;
  coinPoolAssetKey: PublicKey;
  pcPoolAssetKey: PublicKey;
  vaultSignerKey: PublicKey;
  splTokenProgramId: PublicKey;
  dexProgramKey: PublicKey;
  srmReferrerKey: PublicKey | null;
  poolSeed: string;
};

export const SettleFundsDecode = type({
  poolSeed: string(),
  pcIndex: number(),
  orderId: optional(number()),
});

export const decodeSettleFunds = (ix: TransactionInstruction): SettleFunds => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.SettleFunds),
    SettleFundsDecode
  );

  const settleFunds: SettleFunds = {
    programId: ix.programId,
    market: ix.keys[0].pubkey,
    openOrdersKey: ix.keys[1].pubkey,
    poolKey: ix.keys[2].pubkey,
    poolMintKey: ix.keys[3].pubkey,
    coinVaultKey: ix.keys[4].pubkey,
    pcVaultKey: ix.keys[5].pubkey,
    coinPoolAssetKey: ix.keys[6].pubkey,
    pcPoolAssetKey: ix.keys[7].pubkey,
    vaultSignerKey: ix.keys[8].pubkey,
    splTokenProgramId: ix.keys[9].pubkey,
    dexProgramKey: ix.keys[10].pubkey,
    srmReferrerKey: ix.keys[11]?.pubkey,
    poolSeed: decoded.poolSeed,
  };
  return settleFunds;
};

export type Redeem = {
  splTokenProgramId: PublicKey;
  programId: PublicKey;
  mintKey: PublicKey;
  poolKey: PublicKey;
  sourcePoolTokenOwnerKey: PublicKey;
  sourcePoolTokenKey: PublicKey;
  poolSeed: string;
  poolTokenAmount: number;
};

export const RedeemDecode = type({
  poolSeed: string(),
  poolTokenAmount: number(),
});

export const decodeRedeem = (ix: TransactionInstruction): Redeem => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.Redeem),
    RedeemDecode
  );

  const redeem: Redeem = {
    programId: ix.programId,
    splTokenProgramId: ix.keys[0].pubkey,
    mintKey: ix.keys[2].pubkey,
    poolKey: ix.keys[5].pubkey,
    sourcePoolTokenOwnerKey: ix.keys[3].pubkey,
    sourcePoolTokenKey: ix.keys[4].pubkey,
    poolSeed: decoded.poolSeed,
    poolTokenAmount: decoded.poolTokenAmount,
  };

  return redeem;
};

export type CollectFees = {
  splTokenProgramId: PublicKey;
  clockSysvarKey: PublicKey;
  programId: PublicKey;
  poolKey: PublicKey;
  mintKey: PublicKey;
  signalProviderPoolTokenKey: PublicKey;
  bonfidaFeePoolTokenKey: PublicKey;
  bonfidaBnBPTKey: PublicKey;
  poolSeed: string;
};

export const CollectFeesDecode = type({
  poolSeed: string(),
});

export const decodeCollectFees = (ix: TransactionInstruction): CollectFees => {
  const decoded = create(
    decodeInstruction(ix.data, Instruction.CollectFees),
    CollectFeesDecode
  );

  const collectFees: CollectFees = {
    programId: ix.programId,
    splTokenProgramId: ix.keys[0].pubkey,
    clockSysvarKey: ix.keys[1].pubkey,
    poolKey: ix.keys[2].pubkey,
    mintKey: ix.keys[3].pubkey,
    signalProviderPoolTokenKey: ix.keys[4].pubkey,
    bonfidaFeePoolTokenKey: ix.keys[5].pubkey,
    bonfidaBnBPTKey: ix.keys[6].pubkey,
    poolSeed: decoded.poolSeed,
  };

  return collectFees;
};

export const isBonfidaBotInstruction = (
  instruction: TransactionInstruction
) => {
  return instruction.programId.equals(BONFIDABOT_PROGRAM_ID);
};

export const INSTRUCTION_LOOKUP: { [key: number]: string } = {
  0: "Initialize Bot",
  1: "Create Bot",
  2: "Deposit",
  3: "Create Order",
  4: "Cancel Order",
  5: "Settle Funds",
  6: "Redeem",
  7: "Collect Fees",
};

export const parseBonfidaBotInstructionTitle = (
  instruction: TransactionInstruction
): string => {
  const code = instruction.data[0];

  if (!(code in INSTRUCTION_LOOKUP)) {
    throw new Error(`Unrecognized Bonfida Bot instruction code: ${code}`);
  }
  return INSTRUCTION_LOOKUP[code];
};
