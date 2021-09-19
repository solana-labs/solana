/*
    Taken from: https://github.com/metaplex-foundation/metaplex/blob/master/js/packages/common/src/actions/metadata.ts
*/

export type StringPublicKey = string;

export const EDITION = "edition";
export const METADATA_PREFIX = "metadata";

export const MAX_AUCTION_DATA_EXTENDED_SIZE = 8 + 9 + 2 + 200;

export const MAX_NAME_LENGTH = 32;
export const MAX_SYMBOL_LENGTH = 10;
export const MAX_URI_LENGTH = 200;
export const MAX_CREATOR_LIMIT = 5;
export const EDITION_MARKER_BIT_SIZE = 248;
export const MAX_CREATOR_LEN = 32 + 1 + 1;
export const MAX_METADATA_LEN =
  1 +
  32 +
  32 +
  MAX_NAME_LENGTH +
  MAX_SYMBOL_LENGTH +
  MAX_URI_LENGTH +
  MAX_CREATOR_LIMIT * MAX_CREATOR_LEN +
  2 +
  1 +
  1 +
  198;

export enum MetadataKey {
  Uninitialized = 0,
  MetadataV1 = 4,
  EditionV1 = 1,
  MasterEditionV1 = 2,
  MasterEditionV2 = 6,
  EditionMarker = 7,
}

export enum MetadataCategory {
  Audio = "audio",
  Video = "video",
  Image = "image",
  VR = "vr",
  HTML = "html",
}

export type MetadataFile = {
  uri: string;
  type: string;
};

export type FileOrString = MetadataFile | string;

export interface Auction {
  name: string;
  auctionerName: string;
  auctionerLink: string;
  highestBid: number;
  solAmt: number;
  link: string;
  image: string;
}

export interface Artist {
  address?: string;
  name: string;
  link: string;
  image: string;
  itemsAvailable?: number;
  itemsSold?: number;
  about?: string;
  verified?: boolean;

  share?: number;
}

export enum ArtType {
  Master,
  Print,
  NFT,
}
export interface Art {
  url: string;
}

export enum MetaplexKey {
  Uninitialized = 0,
  OriginalAuthorityLookupV1 = 1,
  BidRedemptionTicketV1 = 2,
  StoreV1 = 3,
  WhitelistedCreatorV1 = 4,
  PayoutTicketV1 = 5,
  SafetyDepositValidationTicketV1 = 6,
  AuctionManagerV1 = 7,
  PrizeTrackingTicketV1 = 8,
  SafetyDepositConfigV1 = 9,
  AuctionManagerV2 = 10,
  BidRedemptionTicketV2 = 11,
  AuctionWinnerTokenTypeTrackerV1 = 12,
}
