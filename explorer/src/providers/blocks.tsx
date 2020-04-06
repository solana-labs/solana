import React from "react";
import bs58 from "bs58";
import {
  Connection,
  Transaction,
  TransferParams,
  SystemProgram,
  SystemInstruction,
  CreateAccountParams
} from "@solana/web3.js";
import { useCluster, ClusterStatus } from "./cluster";
import { useTransactions } from "./transactions";

export enum Status {
  Checking,
  CheckFailed,
  Success
}

export interface TransactionDetails {
  transaction: Transaction;
  transfers: Array<TransferParams>;
  creates: Array<CreateAccountParams>;
}

type Transactions = { [signature: string]: TransactionDetails };
export interface Block {
  status: Status;
  transactions?: Transactions;
}

export type Blocks = { [slot: number]: Block };
interface State {
  blocks: Blocks;
}

export enum ActionType {
  Update,
  Add,
  Remove
}

interface Update {
  type: ActionType.Update;
  slot: number;
  status: Status;
  transactions?: Transactions;
}

interface Add {
  type: ActionType.Add;
  slots: number[];
}

interface Remove {
  type: ActionType.Remove;
  slots: number[];
}

type Action = Update | Add | Remove;
type Dispatch = (action: Action) => void;

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case ActionType.Add: {
      if (action.slots.length === 0) return state;
      const blocks = { ...state.blocks };
      action.slots.forEach(slot => {
        if (!blocks[slot]) {
          blocks[slot] = {
            status: Status.Checking
          };
        }
      });
      return { ...state, blocks };
    }
    case ActionType.Remove: {
      if (action.slots.length === 0) return state;
      const blocks = { ...state.blocks };
      action.slots.forEach(slot => {
        delete blocks[slot];
      });
      return { ...state, blocks };
    }
    case ActionType.Update: {
      let block = state.blocks[action.slot];
      if (block) {
        block = {
          ...block,
          status: action.status,
          transactions: action.transactions
        };
        const blocks = {
          ...state.blocks,
          [action.slot]: block
        };
        return { ...state, blocks };
      }
      break;
    }
  }
  return state;
}

const StateContext = React.createContext<State | undefined>(undefined);
const DispatchContext = React.createContext<Dispatch | undefined>(undefined);

type BlocksProviderProps = { children: React.ReactNode };
export function BlocksProvider({ children }: BlocksProviderProps) {
  const [state, dispatch] = React.useReducer(reducer, { blocks: {} });

  const { transactions } = useTransactions();
  const { status, url } = useCluster();

  // Filter blocks for current transaction slots
  React.useEffect(() => {
    if (status !== ClusterStatus.Connected) return;

    const remove: number[] = [];
    const txSlots = transactions
      .map(tx => tx.slot)
      .filter(x => x)
      .reduce((set, slot) => set.add(slot), new Set());
    Object.keys(state.blocks).forEach(blockKey => {
      const slot = parseInt(blockKey);
      if (!txSlots.has(slot)) {
        remove.push(slot);
      }
    });

    dispatch({ type: ActionType.Remove, slots: remove });

    const fetchSlots = new Set<number>();
    transactions.forEach(tx => {
      if (tx.slot && tx.confirmations === "max" && !state.blocks[tx.slot])
        fetchSlots.add(tx.slot);
    });

    const fetchList: number[] = [];
    fetchSlots.forEach(s => fetchList.push(s));
    dispatch({ type: ActionType.Add, slots: fetchList });

    fetchSlots.forEach(slot => {
      fetchBlock(dispatch, slot, url);
    });
  }, [transactions]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>
        {children}
      </DispatchContext.Provider>
    </StateContext.Provider>
  );
}

function decodeTransfers(tx: Transaction) {
  const transferInstructions = tx.instructions
    .filter(ix => ix.programId.equals(SystemProgram.programId))
    .filter(ix => SystemInstruction.decodeInstructionType(ix) === "Transfer");

  let transfers: TransferParams[] = [];
  transferInstructions.forEach(ix => {
    try {
      transfers.push(SystemInstruction.decodeTransfer(ix));
    } catch (err) {
      console.error(ix, err);
    }
  });
  return transfers;
}

function decodeCreates(tx: Transaction) {
  const createInstructions = tx.instructions
    .filter(ix => ix.programId.equals(SystemProgram.programId))
    .filter(ix => SystemInstruction.decodeInstructionType(ix) === "Create");

  let creates: CreateAccountParams[] = [];
  createInstructions.forEach(ix => {
    try {
      creates.push(SystemInstruction.decodeCreateAccount(ix));
    } catch (err) {
      console.error(ix, err);
    }
  });
  return creates;
}

async function fetchBlock(dispatch: Dispatch, slot: number, url: string) {
  dispatch({
    type: ActionType.Update,
    status: Status.Checking,
    slot
  });

  let status;
  let transactions: Transactions = {};
  try {
    const block = await new Connection(url).getConfirmedBlock(slot);
    block.transactions.forEach(({ transaction }) => {
      const signature = transaction.signature;
      if (signature) {
        const sig = bs58.encode(signature);
        transactions[sig] = {
          transaction,
          transfers: decodeTransfers(transaction),
          creates: decodeCreates(transaction)
        };
      }
    });
    status = Status.Success;
  } catch (error) {
    console.error("Failed to fetch confirmed block", error);
    status = Status.CheckFailed;
  }
  dispatch({ type: ActionType.Update, status, slot, transactions });
}

export function useBlocks() {
  const context = React.useContext(StateContext);
  if (!context) {
    throw new Error(`useBlocks must be used within a BlocksProvider`);
  }
  return context;
}
