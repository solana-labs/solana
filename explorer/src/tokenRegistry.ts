import { Cluster } from "providers/cluster";

export type TokenDetails = {
  name: string;
  symbol: string;
  logo?: string;
  icon?: string;
  website?: string;
};

function get(address: string, cluster: Cluster): TokenDetails | undefined {
  if (cluster === Cluster.MainnetBeta) return MAINNET_TOKENS[address];
}

function all(cluster: Cluster) {
  if (cluster === Cluster.MainnetBeta) return MAINNET_TOKENS;
  return {};
}

export const TokenRegistry = {
  get,
  all,
};

const MAINNET_TOKENS: { [key: string]: TokenDetails } = {
  SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt: {
    name: "Serum",
    symbol: "SRM",
    logo: "/tokens/serum-64.png",
    icon: "/tokens/serum-32.png",
    website: "https://projectserum.com",
  },
  MSRMcoVyrFxnSgo5uXwone5SKcGhT1KEJMFEkMEWf9L: {
    name: "MegaSerum",
    symbol: "MSRM",
    logo: "/tokens/serum-64.png",
    icon: "/tokens/serum-32.png",
    website: "https://projectserum.com",
  },
  EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v: {
    symbol: "USDC",
    name: "USD Coin",
    logo: "/tokens/usdc.svg",
    icon: "/tokens/usdc.svg",
    website: "https://www.centre.io/",
  },
  "9S4t2NEAiJVMvPdRYKVrfJpBafPBLtvbvyS3DecojQHw": {
    symbol: "FRONT",
    name: "Wrapped FRONT",
    logo: "/tokens/front.svg",
    icon: "/tokens/front.svg",
    website: "https://frontier.xyz/",
  },
  "9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E": {
    symbol: "BTC",
    name: "Wrapped Bitcoin",
    logo: "/tokens/bitcoin.svg",
    icon: "/tokens/bitcoin.svg",
  },
  "2FPyTwcZLUg1MDrwsyoP4D6s1tM7hAkHYRjkNb5w6Pxk": {
    symbol: "ETH",
    name: "Wrapped Ethereum",
    logo: "/tokens/ethereum.svg",
    icon: "/tokens/ethereum.svg",
  },
  AGFEad2et2ZJif9jaGpdMixQqvW5i81aBdvKe7PHNfz3: {
    symbol: "FTT",
    name: "Wrapped FTT",
    logo: "/tokens/ftt.svg",
    icon: "/tokens/ftt.svg",
  },
  "3JSf5tPeuscJGtaCp5giEiDhv51gQ4v3zWg8DGgyLfAB": {
    symbol: "YFI",
    name: "Wrapped YFI",
    logo: "/tokens/yfi.svg",
    icon: "/tokens/yfi.svg",
  },
  CWE8jPTUYhdCTZYWPTe1o5DFqfdjzWKc9WKz6rSjQUdG: {
    symbol: "LINK",
    name: "Wrapped Chainlink",
    logo: "/tokens/link.svg",
    icon: "/tokens/link.svg",
  },
  Ga2AXHpfAF6mv2ekZwcsJFqu7wB4NV331qNH7fW9Nst8: {
    symbol: "XRP",
    name: "Wrapped XRP",
    logo: "/tokens/xrp.svg",
    icon: "/tokens/xrp.svg",
  },
  BQcdHdAQW1hczDbBi9hiegXAR7A98Q9jx3X3iBBBDiq4: {
    symbol: "USDT",
    name: "Wrapped USDT",
    logo: "/tokens/usdt.svg",
    icon: "/tokens/usdt.svg",
  },
  BXXkv6z8ykpG1yuvUDPgh732wzVHB69RnB9YgSYh3itW: {
    symbol: "USDC",
    name: "Wrapped USDC",
  },
  So11111111111111111111111111111111111111112: {
    symbol: "SAFE",
    name: "Wrapped SAFE",
  },
  SF3oTvfWzEP3DTwGSvUXRrGTvr75pdZNnBLAH9bzMuX: {
    symbol: "SXP",
    name: "Wrapped Swipe",
    logo: "/tokens/sxp.svg",
    icon: "/tokens/sxp.svg",
  },
  BtZQfWqDGbk9Wf2rXEiWyQBdBY1etnUUn6zEphvVS7yN: {
    symbol: "HGET",
    name: "Wrapped Hedget",
  },
  "873KLxCbz7s9Kc4ZzgYRtNmhfkQrhfyWGZJBmyCbC3ei": {
    symbol: "UBXT",
    name: "Wrapped Upbots",
  },
  CsZ5LZkDS7h9TDKjrbL7VAwQZ9nsRu8vJLhRYfmGaN8K: {
    symbol: "ALEPH",
    name: "Wrapped Aleph",
  },
  "5Fu5UUgbjpUvdBveb3a1JTNirL8rXtiYeSMWvKjtUNQv": {
    symbol: "CREAM",
    name: "Wrapped Cream Finance",
    logo: "/tokens/cream.svg",
    icon: "/tokens/cream.svg",
  },
  HqB7uswoVg4suaQiDP3wjxob1G5WdZ144zhdStwMCq7e: {
    symbol: "HNT",
    name: "Wrapped Helium",
  },
  AR1Mtgh7zAtxuxGd2XPovXPVjcSdY3i4rQYisNadjfKy: {
    symbol: "SUSHI",
    name: "Wrapped Sushi",
  },
  AcstFzGGawvvdVhYV9bftr7fmBHbePUjhv53YK1W3dZo: {
    symbol: "LSD",
    name: "LSD",
    website: "https://solible.com",
  },
  "91fSFQsPzMLat9DHwLdQacW3i3EGnWds5tA5mt7yLiT9": {
    symbol: "Unlimited Energy",
    name: "Unlimited Energy",
    website: "https://solible.com",
  },
  "29PEpZeuqWf9tS2gwCjpeXNdXLkaZSMR2s1ibkvGsfnP": {
    symbol: "Need for Speed",
    name: "Need for Speed",
    website: "https://solible.com",
  },
  HsY8PNar8VExU335ZRYzg89fX7qa4upYu6vPMPFyCDdK: {
    symbol: "ADOR OPENS",
    name: "ADOR OPENS",
    website: "https://solible.com",
  },
  EDP8TpLJ77M3KiDgFkZW4v4mhmKJHZi9gehYXenfFZuL: {
    symbol: "CMS - Rare",
    name: "CMS - Rare",
    website: "https://solible.com",
  },
  BrUKFwAABkExb1xzYU4NkRWzjBihVQdZ3PBz4m5S8if3: {
    symbol: "Tesla",
    name: "Tesla",
    website: "https://solible.com",
  },
  "9CmQwpvVXRyixjiE3LrbSyyopPZohNDN1RZiTk8rnXsQ": {
    symbol: "DeceFi",
    name: "DeceFi",
    website: "https://solible.com",
  },
  F6ST1wWkx2PeH45sKmRxo1boyuzzWCfpnvyKL4BGeLxF: {
    symbol: "Power User",
    name: "Power User",
    website: "https://solible.com",
  },
  dZytJ7iPDcCu9mKe3srL7bpUeaR3zzkcVqbtqsmxtXZ: {
    symbol: "VIP Member",
    name: "VIP Member",
    website: "https://solible.com",
  },
  "8T4vXgwZUWwsbCDiptHFHjdfexvLG9UP8oy1psJWEQdS": {
    symbol: "Uni Christmas",
    name: "Uni Christmas",
    website: "https://solible.com",
  },
  EjFGGJSyp9UDS8aqafET5LX49nsG326MeNezYzpiwgpQ: {
    symbol: "BNB",
    name: "BNB",
    website: "https://solible.com",
  },
  FkmkTr4en8CXkfo9jAwEMov6PVNLpYMzWr3Udqf9so8Z: {
    symbol: "Seldom",
    name: "Seldom",
    website: "https://solible.com",
  },
  "2gn1PJdMAU92SU5inLSp4Xp16ZC5iLF6ScEi7UBvp8ZD": {
    symbol: "Satoshi Closeup",
    name: "Satoshi Closeup",
    website: "https://solible.com",
  },
  "7mhZHtPL4GFkquQR4Y6h34Q8hNkQvGc1FaNtyE43NvUR": {
    symbol: "Satoshi GB",
    name: "Satoshi GB",
    website: "https://solible.com",
  },
  "8RoKfLx5RCscbtVh8kYb81TF7ngFJ38RPomXtUREKsT2": {
    symbol: "Satoshi OG",
    name: "Satoshi OG",
    website: "https://solible.com",
  },
  "9rw5hyDngBQ3yDsCRHqgzGHERpU2zaLh1BXBUjree48J": {
    symbol: "Satoshi BTC",
    name: "Satoshi BTC",
    website: "https://solible.com",
  },
  AiD7J6D5Hny5DJB1MrYBc2ePQqy2Yh4NoxWwYfR7PzxH: {
    symbol: "Satoshi GB",
    name: "Satoshi GB",
    website: "https://solible.com",
  },
  bxiA13fpU1utDmYuUvxvyMT8odew5FEm96MRv7ij3eb: {
    symbol: "Satoshi",
    name: "Satoshi",
    website: "https://solible.com",
  },
  GoC24kpj6TkvjzspXrjSJC2CVb5zMWhLyRcHJh9yKjRF: {
    symbol: "Satoshi Closeup",
    name: "Satoshi Closeup",
    website: "https://solible.com",
  },
  oCUduD44ETuZ65bpWdPzPDSnAdreg1sJrugfwyFZVHV: {
    symbol: "Satoshi BTC",
    name: "Satoshi BTC",
    website: "https://solible.com",
  },
  "9Vvre2DxBB9onibwYDHeMsY1cj6BDKtEDccBPWRN215E": {
    symbol: "Satoshi Nakamoto",
    name: "Satoshi Nakamoto",
    website: "https://solible.com",
  },
  "7RpFk44cMTAUt9CcjEMWnZMypE9bYQsjBiSNLn5qBvhP": {
    symbol: "Charles Hoskinson",
    name: "Charles Hoskinson",
    website: "https://solible.com",
  },
  GyRkPAxpd9XrMHcBF6fYHVRSZQvQBwAGKAGQeBPSKzMq: {
    symbol: "SBF",
    name: "SBF",
    website: "https://solible.com",
  },
  AgdBQN2Sy2abiZ2KToWeUsQ9PHdCv95wt6kVWRf5zDkx: {
    symbol: "Bitcoin Tram",
    name: "Bitcoin Tram",
    website: "https://solible.com",
  },
  "7TRzvCqXN8KSXggbSyeEG2Z9YBBhEFmbtmv6FLbd4mmd": {
    symbol: "SRM tee-shirt",
    name: "SRM tee-shirt",
    website: "https://solible.com",
  },
  EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp: {
    symbol: "FIDA",
    name: "FIDA",
    logo: "/tokens/fida.svg",
    icon: "/tokens/fida.svg",
    website: "https://bonfida.com",
  },
  kinXdEcpDQeHPEuQnqmUgtYykqKGVFq6CeVX5iAHJq6: {
    symbol: "KIN",
    name: "KIN",
    logo: "/tokens/kin.svg",
    icon: "/tokens/kin.svg",
    website: "https://kin.org",
  },
  FtgGSFADXBtroxq8VCausXRr2of47QBf5AS1NtZCu4GD: {
    symbol: "BRZ",
    name: "BRZ",
    logo: "/tokens/brz.png",
    icon: "/tokens/brz.png",
    website: "https://brztoken.io",
  },
  MAPS41MDahZ9QdKXhVa4dWB9RuyfV4XqhyAZ8XcYepb: {
    symbol: "MAPS",
    name: "MAPS",
    logo: "/tokens/maps.svg",
    icon: "/tokens/maps.svg",
    website: "https://maps.me/",
  },
  Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB: {
    symbol: "USDT",
    name: "USDT",
    logo: "/tokens/usdt.svg",
    icon: "/tokens/usdt.svg",
    website: "https://tether.to",
  },
};
