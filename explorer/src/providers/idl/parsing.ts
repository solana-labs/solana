import {
  AccountsCoder,
  Instruction,
  InstructionCoder,
} from "@project-serum/anchor";
import { IdlAccountItemParsed } from "utils/instruction";
import {
  Idl,
  IdlAccountItem,
  IdlInstruction,
  IdlType,
  IdlTypeDef,
} from "@project-serum/anchor/dist/cjs/idl";
import { TransactionInstruction } from "@solana/web3.js";

export interface IxDataParsed {
  [fieldName: string]: {
    type: IdlType | undefined;
    value: any;
  };
}

export type AccountDataParsed = IxDataParsed;

export interface InstructionParsed {
  type: IdlInstruction;
  accounts: IdlAccountItemParsed[];
  data: Object;
}

// -------- Accounts parsing

export function parseAccount(
  account: Buffer,
  idl: Idl
): { type: IdlTypeDef; data: any } | undefined {
  if (idl.accounts && idl.accounts.length > 0) {
    const coder = new AccountsCoder(idl);
    const accountType = idl.accounts.find((accountType: any) =>
      (account as Buffer)
        .slice(0, 8)
        .equals(AccountsCoder.accountDiscriminator(accountType.name))
    );
    if (!accountType) return undefined;
    const parsedRaw = coder.decode(accountType.name, account);
    if (!parsedRaw) return undefined;
    return { type: accountType, data: parsedRaw };
  }
}

// -------- End accounts parsing

// -------- Instructions parsing

export function parseIx(
  ix: TransactionInstruction,
  idl: Idl
): InstructionParsed | undefined {
  if (idl.accounts && idl.accounts.length > 0) {
    const ixDataParsed = parseIxData(ix.data, idl);
    if (!ixDataParsed) return undefined;
    const ixType = ixDataParsed.name;
    const ixDef = idl.instructions.find((ixDef) => ixDef.name === ixType);
    if (!ixDef) return undefined;
    if (ixDef.accounts.length !== ix.keys.length) {
      console.log(
        `Mismatching number of accounts between ix ${ix.keys.length} and IDL ${idl.instructions.length}`
      );
      return undefined;
    }
    return {
      type: ixDef,
      data: ixDataParsed.data,
      accounts: matchIxAccountFromIdl(ixDef.accounts, ix, 0),
    };
  }
}

export function parseIxData(ix: Buffer, idl: Idl): Instruction | undefined {
  if (idl.accounts && idl.accounts.length > 0) {
    const coder = new InstructionCoder(idl);
    const parsedRaw = coder.decode(ix);
    if (!parsedRaw) return undefined;
    return parsedRaw;
  }
}

export function matchIxAccountFromIdl(
  accounts: IdlAccountItem[],
  ix: TransactionInstruction,
  keyIndex: number
): IdlAccountItemParsed[] {
  let accountsMatched: IdlAccountItemParsed[] = [];
  accounts.forEach((account) => {
    if ("isMut" in account) {
      accountsMatched.push({
        isSigner: account.isSigner,
        isWritable: account.isMut,
        name: account.name,
        pubkey: ix.keys[keyIndex].pubkey,
      });
      keyIndex++;
    } else {
      accountsMatched.push({
        name: account.name,
        accounts: matchIxAccountFromIdl(accounts, ix, keyIndex),
      });
    }
  });
  return accountsMatched;
}

// -------- End instructions parsing
