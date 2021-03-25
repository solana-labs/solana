import { Connection, VoteAccountStatus } from "@solana/web3.js";
import { Cluster, useCluster } from "providers/cluster";
import React from "react";
import { reportError } from "utils/sentry";

async function fetchVoteAccounts(
  cluster: Cluster,
  url: string,
  setVoteAccounts: React.Dispatch<
    React.SetStateAction<VoteAccountStatus | undefined>
  >
) {
  try {
    const connection = new Connection(url);
    const result = await connection.getVoteAccounts();
    setVoteAccounts(result);
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
}

export function useVoteAccounts() {
  const [voteAccounts, setVoteAccounts] = React.useState<VoteAccountStatus>();
  const { cluster, url } = useCluster();

  return {
    fetchVoteAccounts: () => fetchVoteAccounts(cluster, url, setVoteAccounts),
    voteAccounts,
  };
}
