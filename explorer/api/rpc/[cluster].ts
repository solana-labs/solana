import type { VercelRequest, VercelResponse } from "@vercel/node";
import { clusterUrl, Cluster } from "providers/cluster";

const METHOD_ALLOWLIST = ["getBlock"];

function assertSingularQueryParam(
  putativeSingularParam: string | string[],
  name: string
): asserts putativeSingularParam is string {
  if (Array.isArray(putativeSingularParam)) {
    throw new Error(`There must be only one \`${name}\` query param`);
  }
}

function parseCluster(putativeCluster: string): Cluster {
  switch (putativeCluster) {
    case "devnet":
      return Cluster.Devnet;
    case "testnet":
      return Cluster.Testnet;
    case "mainnet-beta":
      return Cluster.MainnetBeta;
    default:
      throw new Error(
        "`cluster` must be one of `devnet`, `testnet`, or `mainnet-beta`. Received `" +
          putativeCluster +
          "`"
      );
  }
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  const { cluster: putativeCluster } = req.query;
  assertSingularQueryParam(putativeCluster, "cluster");
  const cluster = parseCluster(putativeCluster);
  const requestPayload = JSON.parse(req.body);
  if (!METHOD_ALLOWLIST.includes(requestPayload.method)) {
    res.status(405);
    return;
  }
  const url = clusterUrl(cluster, "" /* customUrl */, false /* cached */);
  const rpcResponse = await fetch(url, {
    body: req.body,
    headers: {
      "Content-Type": "application/json",
      "solana-client": `explorer/${
        process.env.npm_package_version ?? "UNKNOWN"
      }`,
    },
    method: "POST",
  });
  const json = await rpcResponse.json();
  res.setHeader(
    "Cache-Control",
    // max-age=0; the result is always stale.
    // stale-while-revalidate; you may use the stale result while refreshing it in the background.
    // s-maxage; cache this in Vercel, without bothering the Solana RPC
    //           The goal is to cache this result indefinitely, but 1 year is the max value.
    "max-age=0,stale-while-revalidate=31536000,s-maxage=31536000"
  );
  res.status(200).json(json);
}
