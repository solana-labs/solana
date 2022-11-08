import { Connection, LeaderSchedule, VoteAccountStatus } from "@solana/web3.js";
import { Cluster, useCluster } from "providers/cluster";
import React from "react";
import { reportError } from "utils/sentry";

async function fetchLeaderSchedule(
  cluster: Cluster,
  url: string,
  setLeaderSchedule: React.Dispatch<
    React.SetStateAction<LeaderSchedule | undefined>
  >
) {
  try {
    const connection = new Connection(url);
    const result = await connection.getLeaderSchedule();
    setLeaderSchedule(result);
  } catch (error) {
    if (cluster !== Cluster.Custom) {
      reportError(error, { url });
    }
  }
}

export function useLeaderSchedule() {
  const [leaderSchedule, setLeaderSchedule] = React.useState<LeaderSchedule>();
  const { cluster, url } = useCluster();

  return {
    fetchLeaderSchedule: () =>
      fetchLeaderSchedule(cluster, url, setLeaderSchedule),
    leaderSchedule,
  };
}
