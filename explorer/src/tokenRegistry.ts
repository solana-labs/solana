import { Cluster } from "providers/cluster";

export type TokenDetails = {
  name: string;
  symbol: string;
  logo: string;
  icon: string;
  website: string;
};

const ENABLE_DETAILS = !!new URLSearchParams(window.location.search).get(
  "test"
);

function get(address: string, cluster: Cluster): TokenDetails | undefined {
  if (ENABLE_DETAILS && cluster === Cluster.MainnetBeta)
    return MAINNET_TOKENS[address];
}

function all(cluster: Cluster) {
  if (ENABLE_DETAILS && cluster === Cluster.MainnetBeta) return MAINNET_TOKENS;
  return {};
}

export const TokenRegistry = {
  get,
  all,
};

const MAINNET_TOKENS: { [key: string]: TokenDetails } = {
  MSRMmR98uWsTBgusjwyNkE8nDtV79sJznTedhJLzS4B: {
    name: "MegaSerum",
    symbol: "MSRM",
    logo: "/tokens/serum-64.png",
    icon: "/tokens/serum-32.png",
    website: "https://projectserum.com",
  },
};
