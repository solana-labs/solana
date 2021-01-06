import React from "react";

import { Connection } from "@solana/web3.js";
import { useCluster, Cluster } from "providers/cluster";
import { displayTimestamp } from "utils/date";

const CLUSTER_SYNC_INTERVAL = 30000;

export function UnlockAlert() {
  const { cluster, url } = useCluster();
  const [active, setActive] = React.useState(false);
  const [blockTime, setBlockTime] = React.useState<number | null>(null);

  React.useEffect(() => {
    if (!active || !url) {
      return;
    }

    const connection = new Connection(url);
    const getBlockTime = async () => {
      try {
        const epochInfo = await connection.getEpochInfo();
        const blockTime = await connection.getBlockTime(epochInfo.absoluteSlot);
        if (blockTime !== null) {
          setBlockTime(blockTime);
        }
      } catch (error) {}
    };

    getBlockTime();

    const blockTimeInterval = setInterval(getBlockTime, CLUSTER_SYNC_INTERVAL);
    const secondInterval = setInterval(() => {
      setBlockTime((time) => (time !== null ? time + 1 : null));
    }, 1000);

    return () => {
      clearInterval(blockTimeInterval);
      clearInterval(secondInterval);
    };
  }, [active, url]);

  React.useEffect(() => {
    if (cluster !== Cluster.MainnetBeta) {
      return;
    }

    setActive(true);
    return () => {
      setActive(false);
    };
  }, [setActive, cluster]);

  if (cluster !== Cluster.MainnetBeta || blockTime === null) {
    return null;
  }

  return (
    <div className="alert alert-secondary text-center">
      <p>
        An unlock event is timed for midnight, January 7th, UTC cluster time.
      </p>
      <p>
        Cluster time is currently{" "}
        <span className="text-monospace">
          {displayTimestamp(blockTime * 1000)}
        </span>{" "}
        and may differ from actual UTC time.
      </p>
      <p className="mb-0">
        More information can be found{" "}
        <a
          href="https://solana.com/transparency"
          className="text-white font-weight-bold"
          rel="noopener noreferrer"
        >
          here
        </a>
        .
      </p>
    </div>
  );
}
