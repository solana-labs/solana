import React from "react";
import {
  SignatureResult,
  ParsedTransaction,
  ParsedInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InitializeDetailsCard } from "./InitializeDetailsCard";
import { DelegateDetailsCard } from "./DelegateDetailsCard";
import { AuthorizeDetailsCard } from "./AuthorizeDetailsCard";
import { SplitDetailsCard } from "./SplitDetailsCard";
import { WithdrawDetailsCard } from "./WithdrawDetailsCard";
import { DeactivateDetailsCard } from "./DeactivateDetailsCard";
import { ParsedInfo } from "validators";
import { reportError } from "utils/sentry";
import { coerce } from "superstruct";
import {
  AuthorizeInfo,
  DeactivateInfo,
  DelegateInfo,
  InitializeInfo,
  SplitInfo,
  WithdrawInfo,
} from "./types";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function StakeDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);

    switch (parsed.type) {
      case "initialize": {
        const info = coerce(parsed.info, InitializeInfo);
        return <InitializeDetailsCard info={info} {...props} />;
      }
      case "delegate": {
        const info = coerce(parsed.info, DelegateInfo);
        return <DelegateDetailsCard info={info} {...props} />;
      }
      case "authorize": {
        const info = coerce(parsed.info, AuthorizeInfo);
        return <AuthorizeDetailsCard info={info} {...props} />;
      }
      case "split": {
        const info = coerce(parsed.info, SplitInfo);
        return <SplitDetailsCard info={info} {...props} />;
      }
      case "withdraw": {
        const info = coerce(parsed.info, WithdrawInfo);
        return <WithdrawDetailsCard info={info} {...props} />;
      }
      case "deactivate": {
        const info = coerce(parsed.info, DeactivateInfo);
        return <DeactivateDetailsCard info={info} {...props} />;
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
