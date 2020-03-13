import React from "react";
import { testnetChannelEndpoint, Connection } from "@solana/web3.js";

enum NetworkStatus {
  Connected,
  Connecting,
  Failure
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

interface State {
  url: string;
  status: NetworkStatus;
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

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

function findGetParameter(parameterName: string): string | null {
  let result = null,
    tmp = [];
  window.location.search
    .substr(1)
    .split("&")
    .forEach(function(item) {
      tmp = item.split("=");
      if (tmp[0] === parameterName) result = decodeURIComponent(tmp[1]);
    });
  return result;
}

function init(url: string): State {
  const networkUrlParam = findGetParameter("networkUrl");
  return {
    url: networkUrlParam || url,
    status: NetworkStatus.Connecting
  };
}

const DEFAULT_URL = testnetChannelEndpoint("stable");
type NetworkProviderProps = { children: React.ReactNode };
function NetworkProvider({ children }: NetworkProviderProps) {
  const [state, dispatch] = React.useReducer(networkReducer, DEFAULT_URL, init);

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

async function updateNetwork(dispatch: Dispatch, newUrl: string) {
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

function useNetwork() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useNetwork must be used within a NetworkProvider`);
  }
  return context;
}

function useNetworkDispatch() {
  const context = React.useContext(DispatchContext);
  if (!context) {
    throw new Error(`useNetworkDispatch must be used within a NetworkProvider`);
  }
  return context;
}

export {
  NetworkProvider,
  useNetwork,
  useNetworkDispatch,
  updateNetwork,
  NetworkStatus
};
