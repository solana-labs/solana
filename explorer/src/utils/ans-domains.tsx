import { PublicKey, Connection } from "@solana/web3.js";
import { useState, useEffect } from "react";
import { Cluster, useCluster } from "providers/cluster";
import { TldParser, NameRecordHeader } from '@onsol/tldparser';
import { DomainInfo } from './name-service';

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

        let userDomains = await Promise.all(
          allDomains.map(async (address) => {

            const domainRecord = await NameRecordHeader.fromAccountAddress(
              connection,
              address,
            );

            // expired or not found
            if (!domainRecord?.owner) return;

            const domainParentNameAccount = await NameRecordHeader.fromAccountAddress(
              connection,
              domainRecord?.parentName,
            );

            const tldRaw = await parser.getTldFromParentAccount(
              domainRecord?.parentName,
            );

            // not found
            if (!domainParentNameAccount?.owner) return;

            const domainRaw = await parser.reverseLookupNameAccount(
              address,
              domainParentNameAccount?.owner,
            );
            if (!domainRaw) return;
            const tld = Buffer.from(Array.from(tldRaw.split(',')).map(i => Number(i))).toString()
            const domain = Buffer.from(Array.from(domainRaw?.split(',')).map(i => Number(i)))
            const indexof00 = tld.indexOf('\x00')

            return {
              name: `${domain}${tld.substring(0, indexof00)}`,
              address,
            };
          })
        );

        userDomains = userDomains.filter(element => {
          return element !== undefined;
        });
        // console.log('ans domains', userDomains)
        // @ts-ignore since undefined values will be filtered above.
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
