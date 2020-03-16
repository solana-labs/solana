import React from "react";
import { testnetChannelEndpoint, Connection } from "@solana/web3.js";
import { findGetParameter } from "../utils";

export const DEFAULT_URL = testnetChannelEndpoint("stable");

export enum NetworkStatus {
  Connected,
  Connecting,
  Failure
}

interface State {
  url: string;
  status: NetworkStatus;
}

interface Connecting {
  status: NetworkStatus.Connecting;
  url: string;
}

interface Connected {
  status: NetworkStatus.Connected;
}

interface Failure {
  status: NetworkStatus.Failure;
}

type Action = Connected | Connecting | Failure;
type Dispatch = (action: Action) => void;

function networkReducer(state: State, action: Action): State {
  switch (action.status) {
    case NetworkStatus.Connected:
    case NetworkStatus.Failure: {
      return Object.assign({}, state, { status: action.status });
    }
    case NetworkStatus.Connecting: {
      return { url: action.url, status: action.status };
    }
  }
}

function initState(url: string): State {
  const networkUrlParam = findGetParameter("networkUrl");
  return {
    url: networkUrlParam || url,
    status: NetworkStatus.Connecting
  };
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type NetworkProviderProps = { children: React.ReactNode };
export function NetworkProvider({ children }: NetworkProviderProps) {
  const [state, dispatch] = React.useReducer(
    networkReducer,
    DEFAULT_URL,
    initState
  );

  React.useEffect(() => {
    // Connect to network immediately
    updateNetwork(dispatch, state.url);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export async function updateNetwork(dispatch: Dispatch, newUrl: string) {
  dispatch({
    status: NetworkStatus.Connecting,
    url: newUrl
  });

  try {
    const connection = new Connection(newUrl);
    await connection.getRecentBlockhash();
    dispatch({ status: NetworkStatus.Connected });
  } catch (error) {
    console.error("Failed to update network", error);
    dispatch({ status: NetworkStatus.Failure });
  }
}

export function useNetwork() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useNetwork must be used within a NetworkProvider`);
  }
  return context;
}

export function useNetworkDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(`useNetworkDispatch must be used within a NetworkProvider`);
  }
  return context;
}
