import { PublicKey, Connection } from "@solana/web3.js";
import {
  getFilteredProgramAccounts,
  getHashedName,
  getNameAccountKey,
  getNameOwner,
  NAME_PROGRAM_ID,
  performReverseLookup,
} from "@bonfida/spl-name-service";
import { useState, useEffect } from "react";
import { Cluster, useCluster } from "providers/cluster";

// Address of the SOL TLD
const SOL_TLD_AUTHORITY = new PublicKey(
  "58PwtjSDuFHuUkYjH9BYnnQKHfwo9reZhC2zMJv9JPkx"
);

export interface DomainInfo {
  name: string;
  address: PublicKey;
}
export const hasDomainSyntax = (value: string) => {
  return value.length > 4 && value.substring(value.length - 4) === ".sol";
};

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

// returns non empty wallet string if a given .sol domain is owned by a wallet
export async function getDomainInfo(domain: string, connection: Connection) {
  const domainKey = await getDomainKey(
    domain.slice(0, -4), // remove .sol
    undefined,
    SOL_TLD_AUTHORITY
  );
  try {
    const registry = await getNameOwner(connection, domainKey);
    return registry && registry.registry.owner
      ? {
          owner: registry.registry.owner.toString(),
          address: domainKey.toString(),
        }
      : null;
  } catch {
    return null;
  }
}

async function getUserDomainAddresses(
  connection: Connection,
  userAddress: PublicKey
): Promise<PublicKey[]> {
  const filters = [
    // parent
    {
      memcmp: {
        offset: 0,
        bytes: SOL_TLD_AUTHORITY.toBase58(),
      },
    },
    // owner
    {
      memcmp: {
        offset: 32,
        bytes: userAddress.toBase58(),
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

export const useUserDomains = (
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
        const userDomainAddresses = await getUserDomainAddresses(
          connection,
          userAddress
        );
        const userDomains = await Promise.all(
          userDomainAddresses.map(async (address) => {
            const domainName = await performReverseLookup(connection, address);
            return {
              name: `${domainName}.sol`,
              address,
            };
          })
        );
        userDomains.sort((a, b) => a.name.localeCompare(b.name));
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
