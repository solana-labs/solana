import React from "react";
import { Connection } from "@solana/web3.js";
import { useCluster, Cluster } from "providers/cluster";
import {
  DashboardInfo,
  DashboardInfoActionType,
  dashboardInfoReducer,
} from "./solanaDashboardInfo";
import {
  PerformanceInfo,
  PerformanceInfoActionType,
  performanceInfoReducer,
} from "./solanaPerformanceInfo";
import { reportError } from "utils/sentry";

export const PERF_UPDATE_SEC = 5;
export const SAMPLE_HISTORY_HOURS = 6;
export const PERFORMANCE_SAMPLE_INTERVAL = 60000;
export const TRANSACTION_COUNT_INTERVAL = 5000;
export const NON_VOTE_COUNT_INTERVAL = 10000;
export const EPOCH_INFO_INTERVAL = 2000;
export const BLOCK_TIME_INTERVAL = 5000;
export const LOADING_TIMEOUT = 10000;

export enum ClusterStatsStatus {
  Loading,
  Ready,
  Error,
}

const initialPerformanceInfo: PerformanceInfo = {
  status: ClusterStatsStatus.Loading,
  avgTps: 0,
  nonVoteTps: 0,
  recordedNonVoteCount: 0,
  historyMaxTps: 0,
  perfHistory: {
    short: [],
    medium: [],
    long: [],
  },
  transactionCount: 0,
};

const initialDashboardInfo: DashboardInfo = {
  status: ClusterStatsStatus.Loading,
  avgSlotTime_1h: 0,
  avgSlotTime_1min: 0,
  epochInfo: {
    absoluteSlot: 0,
    blockHeight: 0,
    epoch: 0,
    slotIndex: 0,
    slotsInEpoch: 0,
  },
};

type SetActive = React.Dispatch<React.SetStateAction<boolean>>;
const StatsProviderContext = React.createContext<
  | {
      setActive: SetActive;
      setTimedOut: Function;
      retry: Function;
      active: boolean;
    }
  | undefined
>(undefined);

type DashboardState = { info: DashboardInfo };
const DashboardContext = React.createContext<DashboardState | undefined>(
  undefined
);

type PerformanceState = { info: PerformanceInfo };
const PerformanceContext = React.createContext<PerformanceState | undefined>(
  undefined
);

type Props = { children: React.ReactNode };

function getConnection(url: string): Connection | undefined {
  try {
    return new Connection(url);
  } catch (error) {}
}

export function SolanaClusterStatsProvider({ children }: Props) {
  const { cluster, url } = useCluster();
  const [active, setActive] = React.useState(false);
  const [dashboardInfo, dispatchDashboardInfo] = React.useReducer(
    dashboardInfoReducer,
    initialDashboardInfo
  );
  const [performanceInfo, dispatchPerformanceInfo] = React.useReducer(
    performanceInfoReducer,
    initialPerformanceInfo
  );

  React.useEffect(() => {
    if (!active || !url) return;

    const connection = getConnection(url);

    if (!connection) return;

    let lastSlot: number | null = null;

    const getPerformanceSamples = async () => {
      try {
        const samples = await connection.getRecentPerformanceSamples(
          60 * SAMPLE_HISTORY_HOURS
        );

        if (samples.length < 1) {
          // no samples to work with (node has no history).
          return; // we will allow for a timeout instead of throwing an error
        }

        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetPerfSamples,
          data: samples,
        });

        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetPerfSamples,
          data: samples,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        if (error instanceof Error) {
          dispatchPerformanceInfo({
            type: PerformanceInfoActionType.SetError,
            data: error.toString(),
          });
          dispatchDashboardInfo({
            type: DashboardInfoActionType.SetError,
            data: error.toString(),
          });
        }
        setActive(false);
      }
    };

    const getTransactionCount = async () => {
      try {
        const transactionCount = await connection.getTransactionCount();
        dispatchPerformanceInfo({
          type: PerformanceInfoActionType.SetTransactionCount,
          data: transactionCount,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        if (error instanceof Error) {
          dispatchPerformanceInfo({
            type: PerformanceInfoActionType.SetError,
            data: error.toString(),
          });
        }
        setActive(false);
      }
    };

    type BlockNonVoteTxs = {
      txs: number;
      blockTime: number;
      slot: number;
    };

    /**
     * getNonVoteTpsCount gets the 10 last slots and counts the number of
     * transactions with a compute budget larger than 0.
     *
     * Then the blocks and number of transactons is grouped by the block time.
     * We then remove the first and last blockTime as it's not guaranteed that we recorded
     * all txs inside the second.
     *
     * Finally the number of txs over the blocktimes are counted and divided by the number of
     * blocktimes. This in turn is one calculation of tps
     */
    const getNonVoteTpsCount = async () => {
      try {
        const lastBlocks = 10;
        const slot = await connection.getSlot("finalized");
        // get last 10 slots
        const slots = Array.from(
          { length: lastBlocks },
          (v, k) => ((slot - lastBlocks) as number) + k
        );

        // get the number of txs per slot
        const blockNonVoteTxs = (
          await Promise.all(
            slots.map(async (slot) => {
              // get block
              try {
                const block = await connection.getBlock(slot, {
                  maxSupportedTransactionVersion: 0,
                });
                // count non-vote transactions
                const nonVoteTx = block?.transactions.reduce(
                  (prev, next) =>
                    next.meta?.computeUnitsConsumed !== 0 ? prev + 1 : prev,
                  0
                );
                return {
                  txs: nonVoteTx,
                  blockTime: block?.blockTime,
                  slot: slot,
                };
              } catch {
                return undefined;
              }
            })
          )
        ).filter((i) => i !== undefined) as BlockNonVoteTxs[];

        // group by blocktime
        const groupedByBlockTime = blockNonVoteTxs.reduce((prev, next) => {
          const { blockTime } = next;
          (prev as any)[blockTime] = (prev as any)[blockTime] ?? [];
          (prev as any)[blockTime].push(next);
          return prev;
        }, {}) as Record<number, BlockNonVoteTxs[]>;

        // sum the number of txs over each blocktime, this is an approx tps
        const slicedGrouped = Object.keys(groupedByBlockTime)
          .map((time) =>
            groupedByBlockTime[parseInt(time)].reduce((p, n) => p + n.txs, 0)
          )
          .slice(1, -1);
        // only consider slicedGroup of length >= 3
        if (slicedGrouped.length > 0) {
          const numSeconds = slicedGrouped.length;
          const totalTxs = slicedGrouped.reduce((p, n) => p + n);

          // final calculation
          const tps = totalTxs / numSeconds;
          if (tps) {
            dispatchPerformanceInfo({
              type: PerformanceInfoActionType.SetTransactionNonVoteTps,
              data: tps,
            });
          }
        }
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        if (error instanceof Error) {
          dispatchPerformanceInfo({
            type: PerformanceInfoActionType.SetError,
            data: error.toString(),
          });
        }
        setActive(false);
      }
    };

    const getEpochInfo = async () => {
      try {
        const epochInfo = await connection.getEpochInfo();
        lastSlot = epochInfo.absoluteSlot;
        dispatchDashboardInfo({
          type: DashboardInfoActionType.SetEpochInfo,
          data: epochInfo,
        });
      } catch (error) {
        if (cluster !== Cluster.Custom) {
          reportError(error, { url });
        }
        if (error instanceof Error) {
          dispatchDashboardInfo({
            type: DashboardInfoActionType.SetError,
            data: error.toString(),
          });
        }
        setActive(false);
      }
    };

    const getBlockTime = async () => {
      if (lastSlot) {
        try {
          const blockTime = await connection.getBlockTime(lastSlot);
          if (blockTime !== null) {
            dispatchDashboardInfo({
              type: DashboardInfoActionType.SetLastBlockTime,
              data: {
                slot: lastSlot,
                blockTime: blockTime * 1000,
              },
            });
          }
        } catch (error) {
          // let this fail gracefully
        }
      }
    };

    const performanceInterval = setInterval(
      getPerformanceSamples,
      PERFORMANCE_SAMPLE_INTERVAL
    );
    const transactionCountInterval = setInterval(
      getTransactionCount,
      TRANSACTION_COUNT_INTERVAL
    );

    const nonVoteCountInterval = setInterval(
      getNonVoteTpsCount,
      NON_VOTE_COUNT_INTERVAL
    );
    const epochInfoInterval = setInterval(getEpochInfo, EPOCH_INFO_INTERVAL);
    const blockTimeInterval = setInterval(getBlockTime, BLOCK_TIME_INTERVAL);

    getPerformanceSamples();
    getTransactionCount();
    getNonVoteTpsCount();
    (async () => {
      await getEpochInfo();
      await getBlockTime();
    })();

    return () => {
      clearInterval(performanceInterval);
      clearInterval(transactionCountInterval);
      clearInterval(nonVoteCountInterval);
      clearInterval(epochInfoInterval);
      clearInterval(blockTimeInterval);
    };
  }, [active, cluster, url]);

  // Reset when cluster changes
  React.useEffect(() => {
    return () => {
      resetData();
    };
  }, [url]);

  function resetData() {
    dispatchDashboardInfo({
      type: DashboardInfoActionType.Reset,
      data: initialDashboardInfo,
    });
    dispatchPerformanceInfo({
      type: PerformanceInfoActionType.Reset,
      data: initialPerformanceInfo,
    });
  }

  const setTimedOut = React.useCallback(() => {
    dispatchDashboardInfo({
      type: DashboardInfoActionType.SetError,
      data: "Cluster stats timed out",
    });
    dispatchPerformanceInfo({
      type: PerformanceInfoActionType.SetError,
      data: "Cluster stats timed out",
    });
    console.error("Cluster stats timed out");
    setActive(false);
  }, []);

  const retry = React.useCallback(() => {
    resetData();
    setActive(true);
  }, []);

  return (
    <StatsProviderContext.Provider
      value={{ setActive, setTimedOut, retry, active }}
    >
      <DashboardContext.Provider value={{ info: dashboardInfo }}>
        <PerformanceContext.Provider value={{ info: performanceInfo }}>
          {children}
        </PerformanceContext.Provider>
      </DashboardContext.Provider>
    </StatsProviderContext.Provider>
  );
}

export function useStatsProvider() {
  const context = React.useContext(StatsProviderContext);
  if (!context) {
    throw new Error(`useContext must be used within a StatsProvider`);
  }
  return context;
}

export function useDashboardInfo() {
  const context = React.useContext(DashboardContext);
  if (!context) {
    throw new Error(`useDashboardInfo must be used within a StatsProvider`);
  }
  return context.info;
}

export function usePerformanceInfo() {
  const context = React.useContext(PerformanceContext);
  if (!context) {
    throw new Error(`usePerformanceInfo must be used within a StatsProvider`);
  }
  return context.info;
}
