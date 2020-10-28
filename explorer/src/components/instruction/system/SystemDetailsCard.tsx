import React from "react";
import {
  SignatureResult,
  ParsedInstruction,
  ParsedTransaction,
} from "@solana/web3.js";

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
import { ParsedInfo } from "validators";
import { coerce } from "superstruct";
import { reportError } from "utils/sentry";
import { IX_STRUCTS } from "./types";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
};

export function SystemDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    const info = coerce(parsed.info, IX_STRUCTS[parsed.type]);

    switch (parsed.type) {
      case "createAccount":
        return <CreateDetailsCard info={info} {...props} />;
      case "createAccountWithSeed":
        return <CreateWithSeedDetailsCard info={info} {...props} />;
      case "allocate":
        return <AllocateDetailsCard info={info} {...props} />;
      case "allocateWithSeed":
        return <AllocateWithSeedDetailsCard info={info} {...props} />;
      case "assign":
        return <AssignDetailsCard info={info} {...props} />;
      case "assignWithSeed":
        return <AssignWithSeedDetailsCard info={info} {...props} />;
      case "transfer":
        return <TransferDetailsCard info={info} {...props} />;
      case "advanceNonceAccount":
        return <NonceAdvanceDetailsCard info={info} {...props} />;
      case "withdrawNonceAccount":
        return <NonceWithdrawDetailsCard info={info} {...props} />;
      case "authorizeNonceAccount":
        return <NonceAuthorizeDetailsCard info={info} {...props} />;
      case "initializeNonceAccount":
        return <NonceInitializeDetailsCard info={info} {...props} />;
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
