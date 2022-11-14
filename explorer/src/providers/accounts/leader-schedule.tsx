import React from "react";

import { reportError } from "utils/sentry";
import { Connection, LeaderSchedule } from "@solana/web3.js";
import { Cluster, ClusterStatus, useCluster } from "providers/cluster";
import { Status } from "providers/supply";

type Result = LeaderSchedule | Status | string;

type Dispatch = React.Dispatch<React.SetStateAction<Result>>;

const ResultContext = React.createContext<Result | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type Props = { children: React.ReactNode };
export function LeaderScheduleProvider({ children }: Props) {
  const { status, cluster, url } = useCluster();

  const [result, setResult] = React.useState<Result>(Status.Idle);

  React.useEffect(() => {
    if (result !== Status.Idle) {
      switch (status) {
        case ClusterStatus.Connecting: {
          setResult(Status.Disconnected);
          break;
        }
        case ClusterStatus.Connected: {
          fetchLeaderSchedule(cluster, url, setResult);
          break;
        }
      }
    }
  }, [status, cluster, url]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <ResultContext.Provider value={result}>
      <DispatchContext.Provider value={setResult}>
        {children}
      </DispatchContext.Provider>
    </ResultContext.Provider>
  );
}

async function fetchLeaderSchedule(
  cluster: Cluster,
  url: string,
  setResult: Dispatch
) {
  setResult(Status.Connecting);
  try {
    const connection = new Connection(url);
    const leaderSchedule: LeaderSchedule = await connection.getLeaderSchedule();

    setResult(leaderSchedule);
  } catch (err) {
    if (cluster !== Cluster.Custom) {
      reportError(err, { url });
    }
    setResult("Failed to fetch LeaderSchedule Data");
  }
}

export function useLeaderSchedule() {
  const result = React.useContext(ResultContext);
  if (result === undefined) {
    throw new Error(
      `useLeaderSchedule must be used within a LeaderScheduleProvider`
    );
  }
  return result;
}

export function useFetchLeaderSchedule() {
  const dispatch = React.useContext(DispatchContext);
  if (dispatch === undefined) {
    throw new Error(
      `useFetchLeaderSchedule must be used within a LeaderScheduleProvider`
    );
  }
  const { cluster, url } = useCluster();
  return React.useCallback(() => {
    fetchLeaderSchedule(cluster, url, dispatch);
  }, [dispatch, cluster, url]);
}

// import { Connection, LeaderSchedule, VoteAccountStatus } from "@solana/web3.js";
// import { Cluster, useCluster } from "providers/cluster";
// import React from "react";
// import { reportError } from "utils/sentry";

// async function fetchLeaderSchedule(
//   cluster: Cluster,
//   url: string,
//   setLeaderSchedule: React.Dispatch<
//     React.SetStateAction<LeaderSchedule | undefined>
//   >
// ) {
//   try {

//     setLeaderSchedule(result);
//   } catch (error) {
//     if (cluster !== Cluster.Custom) {
//       reportError(error, { url });
//     }
//   }
// }

// export function useLeaderSchedule() {
//   const [leaderSchedule, setLeaderSchedule] = React.useState<LeaderSchedule>();
//   const { cluster, url } = useCluster();

//   return {
//     fetchLeaderSchedule: () =>
//       fetchLeaderSchedule(cluster, url, setLeaderSchedule),
//     leaderSchedule,
//   };
// }
