import React from "react";

import { AccountBalancePair, Connection } from "@solana/web3.js";
import { useCluster, ClusterStatus } from "./cluster";

type RichList = {
  accounts: AccountBalancePair[];
  totalSupply: number;
  circulatingSupply: number;
};

type State = RichList | boolean | string;

type Dispatch = React.Dispatch<React.SetStateAction<State>>;
const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type Props = { children: React.ReactNode };
export function RichListProvider({ children }: Props) {
  const [state, setState] = React.useState<State>(false);
  const { status, url } = useCluster();

  React.useEffect(() => {
    if (status === ClusterStatus.Connecting) setState(false);
    if (status === ClusterStatus.Connected) fetch(setState, url);
  }, [status, url]);

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={setState}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

async function fetch(dispatch: Dispatch, url: string) {
  dispatch(true);
  try {
    const connection = new Connection(url, "max");
    const supply = (await connection.getSupply()).value;
    const accounts = (
      await connection.getLargestAccounts({ filter: "circulating" })
    ).value;

    // Update state if selected cluster hasn't changed
    dispatch(state => {
      if (!state) return state;
      return {
        accounts,
        totalSupply: supply.total,
        circulatingSupply: supply.circulating
      };
    });
  } catch (err) {
    console.error("Failed to fetch", err);
    dispatch("Failed to fetch top accounts");
  }
}

export function useRichList() {
  const state = React.useContext(StateContext);
  if (state === undefined) {
    throw new Error(`useRichList must be used within a RichListProvider`);
  }
  return state;
}

export function useFetchRichList() {
  const dispatch = React.useContext(DispatchContext);
  if (!dispatch) {
    throw new Error(`useFetchRichList must be used within a RichListProvider`);
  }

  const { url } = useCluster();
  return () => fetch(dispatch, url);
}
