import React from "react";

export enum FetchStatus {
  Fetching,
  FetchFailed,
  Fetched,
}

export type CacheEntry<T> = {
  status: FetchStatus;
  data?: T;
};

export type State<T> = {
  entries: {
    [key: string]: CacheEntry<T>;
  };
  url: string;
};

export enum ActionType {
  Update,
  Clear,
}

export type Update<T> = {
  type: ActionType.Update;
  url: string;
  key: string | number;
  status: FetchStatus;
  data?: T;
};

export type Clear = {
  type: ActionType.Clear;
  url: string;
};

export type Action<T> = Update<T> | Clear;
export type Dispatch<T> = (action: Action<T>) => void;
type Reducer<T, U> = (state: State<T>, action: Action<U>) => State<T>;
type Reconciler<T, U> = (
  entry: T | undefined,
  update: U | undefined
) => T | undefined;

function defaultReconciler<T>(entry: T | undefined, update: T | undefined) {
  if (entry) {
    if (update) {
      return {
        ...entry,
        ...update,
      };
    } else {
      return entry;
    }
  } else {
    return update;
  }
}

function defaultReducer<T>(state: State<T>, action: Action<T>) {
  return reducer(state, action, defaultReconciler);
}

export function useReducer<T>(url: string) {
  return React.useReducer<Reducer<T, T>>(defaultReducer, { url, entries: {} });
}

export function useCustomReducer<T, U>(
  url: string,
  reconciler: Reconciler<T, U>
) {
  const customReducer = React.useMemo(() => {
    return (state: State<T>, action: Action<U>) => {
      return reducer(state, action, reconciler);
    };
  }, [reconciler]);
  return React.useReducer<Reducer<T, U>>(customReducer, { url, entries: {} });
}

export function reducer<T, U>(
  state: State<T>,
  action: Action<U>,
  reconciler: Reconciler<T, U>
): State<T> {
  if (action.type === ActionType.Clear) {
    return { url: action.url, entries: {} };
  } else if (action.url !== state.url) {
    return state;
  }

  switch (action.type) {
    case ActionType.Update: {
      const key = action.key;
      const entry = state.entries[key];
      const entries = {
        ...state.entries,
        [key]: {
          ...entry,
          status: action.status,
          data: reconciler(entry?.data, action.data),
        },
      };
      return { ...state, entries };
    }
  }
}
