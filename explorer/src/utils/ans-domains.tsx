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
import * as BufferLayout from "@solana/buffer-layout";

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

const stringToBuffer = (str: string) =>
  Buffer.from(Array.from(str.split(",")).map((i) => Number(i)));

const tldLayout = BufferLayout.utf8(10, "tld");

const domainLayout = BufferLayout.utf8(32, "domain");

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

            const tldStringBuffer = await parser.getTldFromParentAccount(
              domainRecord?.parentName
            );

            const domainStringBuffer = await parser.reverseLookupNameAccount(
              address,
              domainParentNameAccount?.owner
            );
            // domain not found or might be a subdomain.
            if (!domainStringBuffer) return;

            // remove 0x00 bytes
            const indexof00 = tldStringBuffer.indexOf(',0');
            const tldStringBufferClean = tldStringBuffer.substring(0, indexof00);

            // decodes strings from string buffers.
            // a sentient bug. perhaps due to node version (?)
            const tld = tldLayout.decode(
              stringToBuffer(tldStringBufferClean)
            );
            const domain = domainLayout.decode(
              stringToBuffer(domainStringBuffer)
            );
            userDomains.push({
              name: `${domain}${tld}`,
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
