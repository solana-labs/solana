import { PublicKey, Connection } from "@solana/web3.js";
import { useState, useEffect } from "react";
import { Cluster, useCluster } from "providers/cluster";
import {
  TldParser,
  NameRecordHeader,
  getDomainKey,
  getNameOwner,
} from "@onsol/tldparser";
import { DomainInfo } from "./name-service";
import pLimit from "p-limit";

export const hasANSDomainSyntax = (value: string) => {
  return value.length > 4 && value.split(".").length === 2;
};

// returns owner address and name account address.
export async function getANSDomainOwnerAndAddress(
  domainTld: string,
  connection: Connection
) {
  const derivedDomainKey = await getDomainKey(domainTld.toLowerCase());
  try {
    // returns only non expired domains,
    const owner = await getNameOwner(connection, derivedDomainKey.pubkey);
    return owner
      ? {
          owner: owner.toString(),
          address: derivedDomainKey.pubkey.toString(),
        }
      : null;
  } catch {
    return null;
  }
}

export const useUserANSDomains = (
  userAddress: PublicKey
): [DomainInfo[] | null, boolean] => {
  const { url, cluster } = useCluster();
  const [result, setResult] = useState<DomainInfo[] | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const resolve = async () => {
      // Allow only mainnet and custom
      if (![Cluster.MainnetBeta, Cluster.Custom].includes(cluster)) return;
      const connection = new Connection(url, "confirmed");
      try {
        setLoading(true);

        const parser = new TldParser(connection);
        const allDomains = await parser.getAllUserDomains(userAddress);

        if (!allDomains) {
          return;
        }
        let userDomains: DomainInfo[] = [];
        const limit = pLimit(5);
        let promises = allDomains.map((address) =>
          limit(async () => {
            const domainRecord = await NameRecordHeader.fromAccountAddress(
              connection,
              address
            );

            // expired or not found
            if (!domainRecord?.owner) return;

            const domainParentNameAccount =
              await NameRecordHeader.fromAccountAddress(
                connection,
                domainRecord?.parentName
              );

            // not found
            if (!domainParentNameAccount?.owner) return;

            const tldRaw = await parser.getTldFromParentAccount(
              domainRecord?.parentName
            );

            const domainRaw = await parser.reverseLookupNameAccount(
              address,
              domainParentNameAccount?.owner
            );
            // domain not found or might be a subdomain.
            if (!domainRaw) return;

            const tld = Buffer.from(
              Array.from(tldRaw.split(",")).map((i) => Number(i))
            ).toString();

            const domain = Buffer.from(
              Array.from(domainRaw?.split(",")).map((i) => Number(i))
            );

            const indexof00 = tld.indexOf("\x00");
            userDomains.push({
              name: `${domain}${tld.substring(0, indexof00)}`,
              address,
            });
          })
        );

        await Promise.all(promises);
        setResult(userDomains);
      } catch (err) {
        console.log(`Error fetching user domains ${err}`);
      } finally {
        setLoading(false);
      }
    };
    resolve();
  }, [userAddress, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return [result, loading];
};
