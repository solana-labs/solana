import React from "react";
import {
  SignatureResult,
  ParsedInstruction,
  ParsedTransaction,
} from "@safecoin/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { TransferDetailsCard } from "./TransferDetailsCard";
import { AllocateDetailsCard } from "./AllocateDetailsCard";
import { AllocateWithSeedDetailsCard } from "./AllocateWithSeedDetailsCard";
import { AssignDetailsCard } from "./AssignDetailsCard";
import { AssignWithSeedDetailsCard } from "./AssignWithSeedDetailsCard";
import { CreateDetailsCard } from "./CreateDetailsCard";
import { CreateWithSeedDetailsCard } from "./CreateWithSeedDetailsCard";
import { NonceInitializeDetailsCard } from "./NonceInitializeDetailsCard";
import { NonceAdvanceDetailsCard } from "./NonceAdvanceDetailsCard";
import { NonceWithdrawDetailsCard } from "./NonceWithdrawDetailsCard";
import { NonceAuthorizeDetailsCard } from "./NonceAuthorizeDetailsCard";
import { TransferWithSeedDetailsCard } from "./TransferWithSeedDetailsCard";
import { ParsedInfo } from "validators";
import { coerce } from "superstruct";
import { reportError } from "utils/sentry";
import {
  CreateAccountInfo,
  CreateAccountWithSeedInfo,
  AllocateInfo,
  AllocateWithSeedInfo,
  AssignInfo,
  AssignWithSeedInfo,
  TransferInfo,
  AdvanceNonceInfo,
  AuthorizeNonceInfo,
  InitializeNonceInfo,
  WithdrawNonceInfo,
  TransferWithSeedInfo,
} from "./types";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function SystemDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    switch (parsed.type) {
      case "createAccount": {
        const info = coerce(parsed.info, CreateAccountInfo);
        return <CreateDetailsCard info={info} {...props} />;
      }
      case "createAccountWithSeed": {
        const info = coerce(parsed.info, CreateAccountWithSeedInfo);
        return <CreateWithSeedDetailsCard info={info} {...props} />;
      }
      case "allocate": {
        const info = coerce(parsed.info, AllocateInfo);
        return <AllocateDetailsCard info={info} {...props} />;
      }
      case "allocateWithSeed": {
        const info = coerce(parsed.info, AllocateWithSeedInfo);
        return <AllocateWithSeedDetailsCard info={info} {...props} />;
      }
      case "assign": {
        const info = coerce(parsed.info, AssignInfo);
        return <AssignDetailsCard info={info} {...props} />;
      }
      case "assignWithSeed": {
        const info = coerce(parsed.info, AssignWithSeedInfo);
        return <AssignWithSeedDetailsCard info={info} {...props} />;
      }
      case "transfer": {
        const info = coerce(parsed.info, TransferInfo);
        return <TransferDetailsCard info={info} {...props} />;
      }
      case "advanceNonce": {
        const info = coerce(parsed.info, AdvanceNonceInfo);
        return <NonceAdvanceDetailsCard info={info} {...props} />;
      }
      case "withdrawNonce": {
        const info = coerce(parsed.info, WithdrawNonceInfo);
        return <NonceWithdrawDetailsCard info={info} {...props} />;
      }
      case "authorizeNonce": {
        const info = coerce(parsed.info, AuthorizeNonceInfo);
        return <NonceAuthorizeDetailsCard info={info} {...props} />;
      }
      case "initializeNonce": {
        const info = coerce(parsed.info, InitializeNonceInfo);
        return <NonceInitializeDetailsCard info={info} {...props} />;
      }
      case "transferWithSeed": {
        const info = coerce(parsed.info, TransferWithSeedInfo);
        return <TransferWithSeedDetailsCard info={info} {...props} />;
      }
      default:
        return <UnknownDetailsCard {...props} />;
    }
  } catch (error) {
    reportError(error, {
      signature: props.tx.signatures[0],
    });
    return <UnknownDetailsCard {...props} />;
  }
}
