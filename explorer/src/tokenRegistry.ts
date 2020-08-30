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
  },
  MSRMcoVyrFxnSgo5uXwone5SKcGhT1KEJMFEkMEWf9L: {
    name: "MegaSerum",
    symbol: "MSRM",
  },
  "9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E": {
    symbol: "BTC",
    name: "Wrapped Bitcoin",
  },
  "2FPyTwcZLUg1MDrwsyoP4D6s1tM7hAkHYRjkNb5w6Pxk": {
    symbol: "ETH",
    name: "Wrapped Ethereum",
  },
  AGFEad2et2ZJif9jaGpdMixQqvW5i81aBdvKe7PHNfz3: {
    symbol: "FTT",
    name: "Wrapped FTT",
  },
  "3JSf5tPeuscJGtaCp5giEiDhv51gQ4v3zWg8DGgyLfAB": {
    symbol: "YFI",
    name: "Wrapped YFI",
  },
  CWE8jPTUYhdCTZYWPTe1o5DFqfdjzWKc9WKz6rSjQUdG: {
    symbol: "LINK",
    name: "Wrapped Chainlink",
  },
  Ga2AXHpfAF6mv2ekZwcsJFqu7wB4NV331qNH7fW9Nst8: {
    symbol: "XRP",
    name: "Wrapped XRP",
  },
  BQcdHdAQW1hczDbBi9hiegXAR7A98Q9jx3X3iBBBDiq4: {
    symbol: "USDT",
    name: "Wrapped USDT",
  },
  BXXkv6z8ykpG1yuvUDPgh732wzVHB69RnB9YgSYh3itW: {
    symbol: "USDC",
    name: "Wrapped USDC",
  },
};
