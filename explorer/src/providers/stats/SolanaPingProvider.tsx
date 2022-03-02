import React from "react";
import { Cluster, clusterSlug, useCluster } from "providers/cluster";
import { fetch } from "cross-fetch";
import { useStatsProvider } from "providers/stats/solanaClusterStats";

const FETCH_PING_INTERVAL = 60 * 1000;

function getPingUrl(cluster: Cluster) {
  const slug = clusterSlug(cluster);

  if (slug === "custom") {
    return undefined;
  }

  return `https://ping.solana.com/${slug}/last6hours`;
}

export type PingMetric = {
  submitted: number;
  confirmed: number;
  loss: string;
  mean_ms: number;
  ts: string;
  error: string;
};

export type PingInfo = {
  submitted: number;
  confirmed: number;
  loss: number;
  mean: number;
  timestamp: Date;
};

export enum PingStatus {
  Loading,
  Ready,
  Error,
}

export type PingRollupInfo = {
  status: PingStatus;
  short?: PingInfo[];
  medium?: PingInfo[];
  long?: PingInfo[];
  retry?: Function;
};

const PingContext = React.createContext<PingRollupInfo | undefined>(undefined);

type Props = { children: React.ReactNode };

function downsample(points: PingInfo[], bucketSize: number): PingInfo[] {
  const buckets = [];

  for (let start = 0; start < points.length; start += bucketSize) {
    const summary: PingInfo = {
      submitted: 0,
      confirmed: 0,
      loss: 0,
      mean: 0,
      timestamp: points[start].timestamp,
    };
    for (let i = 0; i < bucketSize; i++) {
      summary.submitted += points[start + i].submitted;
      summary.confirmed += points[start + i].confirmed;
      summary.mean += points[start + i].mean;
    }
    summary.mean = Math.round(summary.mean / bucketSize);
    summary.loss = (summary.submitted - summary.confirmed) / summary.submitted;
    buckets.push(summary);
  }

  return buckets;
}

export function SolanaPingProvider({ children }: Props) {
  const { cluster } = useCluster();
  const { active } = useStatsProvider();
  const [rollup, setRollup] = React.useState<PingRollupInfo | undefined>({
    status: PingStatus.Loading,
  });

  React.useEffect(() => {
    if (!active) {
      return;
    }

    const url = getPingUrl(cluster);

    setRollup({
      status: PingStatus.Loading,
    });

    if (!url) {
      return;
    }

    const fetchPingMetrics = () => {
      fetch(url)
        .then((res) => {
          return res.json();
        })
        .then((body: PingMetric[]) => {
          const points = body
            .map<PingInfo>(
              ({ submitted, confirmed, mean_ms, ts }: PingMetric) => {
                return {
                  submitted,
                  confirmed,
                  loss: (submitted - confirmed) / submitted,
                  mean: mean_ms,
                  timestamp: new Date(ts),
                };
              }
            )
            .reverse();

          const short = points.slice(-30);
          const medium = downsample(points, 4).slice(-30);
          const long = downsample(points, 12);

          setRollup({
            short,
            medium,
            long,
            status: PingStatus.Ready,
          });
        })
        .catch((error) => {
          setRollup({
            status: PingStatus.Error,
            retry: () => {
              setRollup({
                status: PingStatus.Loading,
              });

              fetchPingMetrics();
            },
          });
        });
    };

    const fetchPingInterval = setInterval(() => {
      fetchPingMetrics();
    }, FETCH_PING_INTERVAL);
    fetchPingMetrics();
    return () => {
      clearInterval(fetchPingInterval);
    };
  }, [cluster, active]);

  return <PingContext.Provider value={rollup}>{children}</PingContext.Provider>;
}

export function useSolanaPingInfo() {
  const context = React.useContext(PingContext);
  if (!context) {
    throw new Error(`useContext must be used within a StatsProvider`);
  }
  return context;
}
