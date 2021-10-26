import { PublicKey, Connection } from "@solana/web3.js";
import {
  getHashedName,
  getNameAccountKey,
  NameRegistryState,
  getFilteredProgramAccounts,
  NAME_PROGRAM_ID,
} from "@bonfida/spl-name-service";
import BN from "bn.js";
import { useState, useEffect } from "react";
import { Cluster, useCluster } from "providers/cluster";

// Name auctionning Program ID
export const PROGRAM_ID = new PublicKey(
  "jCebN34bUfdeUYJT13J1yG16XWQpt5PDx6Mse9GUqhR"
);

export interface DomainInfo {
  name: string;
  address: PublicKey;
  class: PublicKey;
}

async function getDomainKey(
  name: string,
  nameClass?: PublicKey,
  nameParent?: PublicKey
) {
  const hashedDomainName = await getHashedName(name);
  const nameKey = await getNameAccountKey(
    hashedDomainName,
    nameClass,
    nameParent
  );
  return nameKey;
}

export async function findOwnedNameAccountsForUser(
  connection: Connection,
  userAccount: PublicKey
): Promise<PublicKey[]> {
  const filters = [
    {
      memcmp: {
        offset: 32,
        bytes: userAccount.toBase58(),
      },
    },
  ];
  const accounts = await getFilteredProgramAccounts(
    connection,
    NAME_PROGRAM_ID,
    filters
  );
  return accounts.map((a) => a.publicKey);
}

export async function performReverseLookup(
  connection: Connection,
  nameAccounts: PublicKey[]
): Promise<DomainInfo[]> {
  let [centralState] = await PublicKey.findProgramAddress(
    [PROGRAM_ID.toBuffer()],
    PROGRAM_ID
  );

  const reverseLookupAccounts = await Promise.all(
    nameAccounts.map((name) => getDomainKey(name.toBase58(), centralState))
  );

  let names = await NameRegistryState.retrieveBatch(
    connection,
    reverseLookupAccounts
  );

  return names
    .map((name) => {
      if (!name?.data) {
        return undefined;
      }
      const nameLength = new BN(name!.data.slice(0, 4), "le").toNumber();
      return {
        name: name.data.slice(4, 4 + nameLength).toString() + ".sol",
        address: name.address,
        class: name.class,
      };
    })
    .filter((e) => !!e) as DomainInfo[];
}

export const useUserDomains = (
  address: PublicKey
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
        const domains = await findOwnedNameAccountsForUser(connection, address);
        let names = await performReverseLookup(connection, domains);
        names.sort((a, b) => {
          return a.name.localeCompare(b.name);
        });
        setResult(names);
      } catch (err) {
        console.log(`Error fetching user domains ${err}`);
      } finally {
        setLoading(false);
      }
    };
    resolve();
  }, [address, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return [result, loading];
};
