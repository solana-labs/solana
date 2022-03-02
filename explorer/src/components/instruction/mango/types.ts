import {
  Config,
  GroupConfig,
  MangoInstructionLayout,
  PerpMarket,
  PerpMarketConfig,
  PerpMarketLayout,
  SpotMarketConfig,
} from "@blockworks-foundation/mango-client";
import { Market } from "@project-serum/serum";
import {
  AccountInfo,
  AccountMeta,
  Connection,
  PublicKey,
  TransactionInstruction,
} from "@solana/web3.js";

// note: mainnet.1 suffices since its a superset of mainnet.0
const mangoGroups = Config.ids().groups.filter(
  (group) => group.name !== "mainnet.0"
);

// caching of account info's by public keys
let accountInfoCache: Record<string, Promise<AccountInfo<Buffer> | null>> = {};
function getAccountInfo(
  clusterUrl: string,
  publicKey: PublicKey
): Promise<AccountInfo<Buffer> | null> {
  if (publicKey.toBase58() in accountInfoCache) {
    return accountInfoCache[publicKey.toBase58()];
  }
  const connection = new Connection(clusterUrl);
  const accountInfoPromise = connection.getAccountInfo(publicKey);
  accountInfoCache[publicKey.toBase58()] = accountInfoPromise;
  return accountInfoPromise;
}

function findGroupConfig(programId: PublicKey): GroupConfig | undefined {
  const filtered = mangoGroups.filter((group) =>
    group.mangoProgramId.equals(programId)
  );
  if (filtered.length) {
    return filtered[0];
  }
}

export const isMangoInstruction = (instruction: TransactionInstruction) => {
  return mangoGroups
    .map((group) => group.mangoProgramId.toBase58())
    .includes(instruction.programId.toBase58());
};

export const parseMangoInstructionTitle = (
  instruction: TransactionInstruction
): string => {
  let decodedInstruction = MangoInstructionLayout.decode(instruction.data, 0);
  return Object.keys(decodedInstruction)[0];
};

export type Deposit = {
  quantity: number;
};

export const decodeDeposit = (ix: TransactionInstruction): Deposit => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const deposit: Deposit = {
    quantity: decoded.Deposit.quantity.toNumber(),
  };
  return deposit;
};

export type AddToBasket = {
  marketIndex: number;
};

export const decodeAddToBasket = (ix: TransactionInstruction): AddToBasket => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const addToBasket: AddToBasket = {
    marketIndex: decoded.AddToBasket.marketIndex.toNumber(),
  };
  return addToBasket;
};

export type Withdraw = {
  quantity: number;
  allowBorrow: String;
};

export const decodeWithdraw = (ix: TransactionInstruction): Withdraw => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const withdraw: Withdraw = {
    quantity: decoded.Withdraw.quantity.toNumber(),
    allowBorrow: decoded.Withdraw.allowBorrow.toString(),
  };
  return withdraw;
};

export type PlaceSpotOrder = {
  side: String;
  limitPrice: number;
  maxBaseQuantity: number;
  maxQuoteQuantity: number;
  selfTradeBehavior: String;
  orderType: String;
  clientId: String;
  limit: String;
};

export const decodePlaceSpotOrder = (
  ix: TransactionInstruction
): PlaceSpotOrder => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const placeSpotOrder: PlaceSpotOrder = {
    side: decoded.PlaceSpotOrder.side.toString(),
    limitPrice: decoded.PlaceSpotOrder.limitPrice.toNumber(),
    maxBaseQuantity: decoded.PlaceSpotOrder.maxBaseQuantity.toNumber(),
    maxQuoteQuantity: decoded.PlaceSpotOrder.maxQuoteQuantity.toNumber(),
    selfTradeBehavior: decoded.PlaceSpotOrder.selfTradeBehavior,
    orderType: decoded.PlaceSpotOrder.orderType.toString(),
    clientId: decoded.PlaceSpotOrder.clientId.toString(),
    limit: decoded.PlaceSpotOrder.limit.toString(),
  };

  return placeSpotOrder;
};

export type CancelSpotOrder = {
  orderId: String;
  side: String;
};

export const decodeCancelSpotOrder = (
  ix: TransactionInstruction
): CancelSpotOrder => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const cancelSpotOrder: CancelSpotOrder = {
    orderId: decoded.CancelSpotOrder.orderId.toString(),
    side: decoded.CancelSpotOrder.side.toString(),
  };
  return cancelSpotOrder;
};

export type PlacePerpOrder = {
  price: number;
  quantity: number;
  clientOrderId: String;
  side: String;
  orderType: String;
  reduceOnly: String;
};

export const decodePlacePerpOrder = (
  ix: TransactionInstruction
): PlacePerpOrder => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const placePerpOrder: PlacePerpOrder = {
    price: decoded.PlacePerpOrder.price.toNumber(),
    quantity: decoded.PlacePerpOrder.quantity.toNumber(),
    clientOrderId: decoded.PlacePerpOrder.clientOrderId.toString(),
    side: decoded.PlacePerpOrder.side.toString(),
    orderType: decoded.PlacePerpOrder.orderType.toString(),
    reduceOnly: decoded.PlacePerpOrder.reduceOnly.toString(),
  };

  return placePerpOrder;
};

export type PlacePerpOrder2 = {
  price: number;
  maxBaseQuantity: number;
  clientOrderId: String;
  side: String;
  orderType: String;
  reduceOnly: String;
  expiryTimestamp: number;
};

export const decodePlacePerpOrder2 = (
  ix: TransactionInstruction
): PlacePerpOrder2 => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const placePerpOrder2: PlacePerpOrder2 = {
    price: decoded.PlacePerpOrder2.price.toNumber(),
    maxBaseQuantity: decoded.PlacePerpOrder2.maxBaseQuantity.toNumber(),
    clientOrderId: decoded.PlacePerpOrder2.clientOrderId.toString(),
    side: decoded.PlacePerpOrder2.side.toString(),
    orderType: decoded.PlacePerpOrder2.orderType.toString(),
    reduceOnly: decoded.PlacePerpOrder2.reduceOnly.toString(),
    expiryTimestamp: decoded.PlacePerpOrder2.expiryTimestamp.toNumber(),
  };

  return placePerpOrder2;
};

export type CancelPerpOrder = {
  orderId: String;
  invalidIdOk: String;
};

export const decodeCancelPerpOrder = (
  ix: TransactionInstruction
): CancelPerpOrder => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const cancelPerpOrder: CancelPerpOrder = {
    orderId: decoded.CancelPerpOrder.orderId.toString(),
    invalidIdOk: decoded.CancelPerpOrder.invalidIdOk.toString(),
  };
  return cancelPerpOrder;
};

export type ChangePerpMarketParams = {
  maintLeverageOption: Boolean;
  maintLeverage: number;
  initLeverageOption: Boolean;
  initLeverage: number;
  liquidationFeeOption: Boolean;
  liquidationFee: number;
  makerFeeOption: Boolean;
  makerFee: number;
  takerFeeOption: Boolean;
  takerFee: number;
  rateOption: Boolean;
  rate: number;
  maxDepthBpsOption: Boolean;
  maxDepthBps: number;
  targetPeriodLengthOption: Boolean;
  targetPeriodLength: number;
  mngoPerPeriodOption: Boolean;
  mngoPerPeriod: number;
};

export const decodeChangePerpMarketParams = (
  ix: TransactionInstruction
): ChangePerpMarketParams => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const changePerpMarketParams: ChangePerpMarketParams = {
    maintLeverageOption: decoded.ChangePerpMarketParams.maintLeverageOption,
    maintLeverage: decoded.ChangePerpMarketParams.maintLeverage.toString(),
    initLeverageOption: decoded.ChangePerpMarketParams.initLeverageOption,
    initLeverage: decoded.ChangePerpMarketParams.initLeverage.toString(),
    liquidationFeeOption: decoded.ChangePerpMarketParams.liquidationFeeOption,
    liquidationFee: decoded.ChangePerpMarketParams.liquidationFee.toString(),
    makerFeeOption: decoded.ChangePerpMarketParams.makerFeeOption,
    makerFee: decoded.ChangePerpMarketParams.makerFee.toString(),
    takerFeeOption: decoded.ChangePerpMarketParams.takerFeeOption,
    takerFee: decoded.ChangePerpMarketParams.takerFee.toString(),
    rateOption: decoded.ChangePerpMarketParams.rateOption,
    rate: decoded.ChangePerpMarketParams.rate.toString(),
    maxDepthBpsOption: decoded.ChangePerpMarketParams.maxDepthBpsOption,
    maxDepthBps: decoded.ChangePerpMarketParams.maxDepthBps.toString(),
    targetPeriodLengthOption:
      decoded.ChangePerpMarketParams.targetPeriodLengthOption,
    targetPeriodLength:
      decoded.ChangePerpMarketParams.targetPeriodLength.toString(),
    mngoPerPeriodOption: decoded.ChangePerpMarketParams.mngoPerPeriodOption,
    mngoPerPeriod: decoded.ChangePerpMarketParams.mngoPerPeriod.toString(),
  };
  return changePerpMarketParams;
};

export type AddSpotMarket = {
  marketIndex: number;
  maintLeverage: number;
  initLeverage: number;
  liquidationFee: number;
  optimalUtil: number;
  optimalRate: number;
  maxRate: number;
};

export const decodeAddSpotMarket = (
  ix: TransactionInstruction
): AddSpotMarket => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const addSpotMarket: AddSpotMarket = {
    marketIndex: decoded.AddSpotMarket.marketIndex.toNumber(),
    maintLeverage: decoded.AddSpotMarket.maintLeverage.toNumber(),
    initLeverage: decoded.AddSpotMarket.initLeverage.toNumber(),
    liquidationFee: decoded.AddSpotMarket.liquidationFee.toNumber(),
    optimalUtil: decoded.AddSpotMarket.optimalUtil.toNumber(),
    optimalRate: decoded.AddSpotMarket.optimalRate.toNumber(),
    maxRate: decoded.AddSpotMarket.maxRate.toNumber(),
  };
  return addSpotMarket;
};

export type AddPerpMarket = {
  marketIndex: number;
  maintLeverage: number;
  initLeverage: number;
  liquidationFee: number;
  makerFee: number;
  takerFee: number;
  baseLotSize: number;
  quoteLotSize: number;
  rate: number;
  maxDepthBps: number;
  targetPeriodLength: number;
  mngoPerPeriod: number;
};

export const decodeAddPerpMarket = (
  ix: TransactionInstruction
): AddPerpMarket => {
  const decoded = MangoInstructionLayout.decode(ix.data, 0);
  const addPerpMarket: AddPerpMarket = {
    marketIndex: decoded.AddPerpMarket.marketIndex.toNumber(),
    maintLeverage: decoded.AddPerpMarket.maintLeverage.toNumber(),
    initLeverage: decoded.AddPerpMarket.initLeverage.toNumber(),
    liquidationFee: decoded.AddPerpMarket.liquidationFee.toNumber(),
    makerFee: decoded.AddPerpMarket.makerFee.toNumber(),
    takerFee: decoded.AddPerpMarket.takerFee.toNumber(),
    baseLotSize: decoded.AddPerpMarket.baseLotSize.toNumber(),
    quoteLotSize: decoded.AddPerpMarket.quoteLotSize.toNumber(),
    rate: decoded.AddPerpMarket.rate.toNumber(),
    maxDepthBps: decoded.AddPerpMarket.maxDepthBps.toNumber(),
    targetPeriodLength: decoded.AddPerpMarket.targetPeriodLength.toNumber(),
    mngoPerPeriod: decoded.AddPerpMarket.mngoPerPeriod.toNumber(),
  };
  return addPerpMarket;
};

export type OrderLotDetails = {
  price: number;
  size: number;
};

////

export function logAllKeys(keys: AccountMeta[]) {
  keys.map((key) => console.log(key.pubkey.toBase58()));
}

export function getSpotMarketFromInstruction(
  ix: TransactionInstruction,
  spotMarket: AccountMeta
): SpotMarketConfig | undefined {
  const groupConfig = findGroupConfig(ix.programId);
  if (groupConfig === undefined) {
    return;
  }
  const spotMarketConfigs = groupConfig.spotMarkets.filter((mangoSpotMarket) =>
    spotMarket.pubkey.equals(mangoSpotMarket.publicKey)
  );
  if (spotMarketConfigs.length) {
    return spotMarketConfigs[0];
  }
}

export async function getSpotMarketFromSpotMarketConfig(
  programId: PublicKey,
  clusterUrl: string,
  mangoSpotMarketConfig: SpotMarketConfig
): Promise<Market | undefined> {
  const connection = new Connection(clusterUrl);
  const groupConfig = findGroupConfig(programId);
  if (groupConfig === undefined) {
    return;
  }
  return await Market.load(
    connection,
    mangoSpotMarketConfig.publicKey,
    undefined,
    groupConfig.serumProgramId
  );
}

export function getPerpMarketFromInstruction(
  ix: TransactionInstruction,
  perpMarket: AccountMeta
): PerpMarketConfig | undefined {
  const groupConfig = findGroupConfig(ix.programId);
  if (groupConfig === undefined) {
    return;
  }
  const perpMarketConfigs = groupConfig.perpMarkets.filter((mangoPerpMarket) =>
    perpMarket.pubkey.equals(mangoPerpMarket.publicKey)
  );
  if (perpMarketConfigs.length) {
    return perpMarketConfigs[0];
  }
}

export async function getPerpMarketFromPerpMarketConfig(
  clusterUrl: string,
  mangoPerpMarketConfig: PerpMarketConfig
): Promise<PerpMarket> {
  const acc = await getAccountInfo(clusterUrl, mangoPerpMarketConfig.publicKey);
  const decoded = PerpMarketLayout.decode(acc?.data);

  return new PerpMarket(
    mangoPerpMarketConfig.publicKey,
    mangoPerpMarketConfig.baseDecimals,
    mangoPerpMarketConfig.quoteDecimals,
    decoded
  );
}

export function spotMarketFromIndex(
  ix: TransactionInstruction,
  marketIndex: number
): String | undefined {
  const groupConfig = findGroupConfig(ix.programId);
  if (groupConfig === undefined) {
    return;
  }
  const spotMarketConfigs = groupConfig.spotMarkets.filter(
    (spotMarketConfig) => spotMarketConfig.marketIndex === marketIndex
  );
  if (!spotMarketConfigs.length) {
    return "UNKNOWN";
  }
  return spotMarketConfigs[0].name;
}
