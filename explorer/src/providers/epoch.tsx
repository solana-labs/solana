import React from "react";
import * as Cache from "providers/cache";
import { Connection, EpochSchedule } from "@solana/web3.js";
import { useCluster, Cluster } from "./cluster";
import { reportError } from "utils/sentry";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched,
}

export enum ActionType {
  Update,
  Clear,
}

type Epoch = {
  firstBlock: number;
  firstTimestamp: number | null;
  lastBlock?: number;
  lastTimestamp: number | null;
};

type State = Cache.State<Epoch>;
type Dispatch = Cache.Dispatch<Epoch>;

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type EpochProviderProps = { children: React.ReactNode };

export function EpochProvider({ children }: EpochProviderProps) {
  const { url } = useCluster();
  const [state, dispatch] = Cache.useReducer<Epoch>(url);

  React.useEffect(() => {
    dispatch({ type: ActionType.Clear, url });
  }, [dispatch, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export function useEpoch(key: number): Cache.CacheEntry<Epoch> | undefined {
  const context = React.useContext(StateContext);

  if (!context) {
    throw new Error(`useEpoch must be used within a EpochProvider`);
  }

  return context.entries[key];
}

export async function fetchEpoch(
  dispatch: Dispatch,
  url: string,
  cluster: Cluster,
  epochSchedule: EpochSchedule,
  currentEpoch: number,
  epoch: number
) {
  dispatch({
    type: ActionType.Update,
    status: FetchStatus.Fetching,
    key: epoch,
    url,
  });

  let status: FetchStatus;
  let data: Epoch | undefined = undefined;

  try {
    const connection = new Connection(url, "confirmed");
    const firstSlot = epochSchedule.getFirstSlotInEpoch(epoch);
    const lastSlot = epochSchedule.getLastSlotInEpoch(epoch);
    const [firstBlock, lastBlock] = await Promise.all([
      (async () => {
        const firstBlocks = await connection.getBlocks(
          firstSlot,
          firstSlot + 100
        );
        return firstBlocks.shift();
      })(),
      (async () => {
        const lastBlocks = await connection.getBlocks(
          Math.max(0, lastSlot - 100),
          lastSlot
        );
        return lastBlocks.pop();
      })(),
    ]);

    if (firstBlock === undefined) {
      throw new Error(
        `failed to find confirmed block at start of epoch ${epoch}`
      );
    } else if (epoch < currentEpoch && lastBlock === undefined) {
      throw new Error(
        `failed to find confirmed block at end of epoch ${epoch}`
      );
    }

    const [firstTimestamp, lastTimestamp] = await Promise.all([
      connection.getBlockTime(firstBlock),
      lastBlock ? connection.getBlockTime(lastBlock) : null,
    ]);

    data = {
      firstBlock,
      lastBlock,
      firstTimestamp,
      lastTimestamp,
    };
    status = FetchStatus.Fetched;
  } catch (err) {
    status = FetchStatus.FetchFailed;
    if (cluster !== Cluster.Custom) {
      reportError(err, { epoch: epoch.toString() });
    }
  }

  dispatch({
    type: ActionType.Update,
    url,
    key: epoch,
    status,
    data,
  });
}

export function useFetchEpoch() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchEpoch must be used within a EpochProvider`);
  }

  const { cluster, url } = useCluster();
  return React.useCallback(
    (key: number, currentEpoch: number, epochSchedule: EpochSchedule) =>
      fetchEpoch(dispatch, url, cluster, epochSchedule, currentEpoch, key),
    [dispatch, cluster, url]
  );
}
