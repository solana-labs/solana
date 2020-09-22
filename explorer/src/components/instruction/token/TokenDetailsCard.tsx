import React from "react";
import { coerce } from "superstruct";
import {
  SignatureResult,
  ParsedTransaction,
  PublicKey,
  ParsedInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import { IX_STRUCTS, TokenInstructionType, IX_TITLES } from "./types";
import { ParsedInfo } from "validators";
import {
  useTokenAccountInfo,
  useMintAccountInfo,
  useFetchAccountInfo,
} from "providers/accounts";
import { normalizeTokenAmount } from "utils";
import { reportError } from "utils/sentry";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
};

export function TokenDetailsCard(props: DetailsProps) {
  try {
    const parsed = coerce(props.ix.parsed, ParsedInfo);
    const { type: rawType, info } = parsed;
    const type = coerce(rawType, TokenInstructionType);
    const title = `Token: ${IX_TITLES[type]}`;
    const coerced = coerce(info, IX_STRUCTS[type] as any);
    return <TokenInstruction title={title} info={coerced} {...props} />;
  } catch (err) {
    reportError(err, {
      signature: props.tx.signatures[0],
    });
    return <UnknownDetailsCard {...props} />;
  }
}

type InfoProps = {
  ix: ParsedInstruction;
  info: any;
  result: SignatureResult;
  index: number;
  title: string;
};

function TokenInstruction(props: InfoProps) {
  const { mintAddress: infoMintAddress, tokenAddress } = React.useMemo(() => {
    let mintAddress: string | undefined;
    let tokenAddress: string | undefined;

    // No sense fetching accounts if we don't need to convert an amount
    if (!("amount" in props.info)) return {};

    if ("mint" in props.info && props.info.mint instanceof PublicKey) {
      mintAddress = props.info.mint.toBase58();
    } else if (
      "account" in props.info &&
      props.info.account instanceof PublicKey
    ) {
      tokenAddress = props.info.account.toBase58();
    } else if (
      "source" in props.info &&
      props.info.source instanceof PublicKey
    ) {
      tokenAddress = props.info.source.toBase58();
    }
    return {
      mintAddress,
      tokenAddress,
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const tokenInfo = useTokenAccountInfo(tokenAddress);
  const mintAddress = infoMintAddress || tokenInfo?.mint.toBase58();
  const mintInfo = useMintAccountInfo(mintAddress);
  const fetchAccountInfo = useFetchAccountInfo();

  React.useEffect(() => {
    if (tokenAddress && !tokenInfo) {
      fetchAccountInfo(new PublicKey(tokenAddress));
    }
  }, [fetchAccountInfo, tokenAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  React.useEffect(() => {
    if (mintAddress && !mintInfo) {
      fetchAccountInfo(new PublicKey(mintAddress));
    }
  }, [fetchAccountInfo, mintAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  const decimals = mintInfo?.decimals;
  const attributes: JSX.Element[] = [];

  buildAttributes(attributes, props.info, decimals);

  return (
    <InstructionCard
      ix={props.ix}
      index={props.index}
      result={props.result}
      title={props.title}
    >
      {attributes}
    </InstructionCard>
  );
}

function buildAttributes(
  attributes: JSX.Element[],
  info: any,
  decimals: number | undefined
) {
  for (let key in info) {
    const value = info[key];
    if (value === undefined) continue;

    let tag;
    let labelSuffix = "";
    if (value instanceof PublicKey) {
      tag = <Address pubkey={value} alignRight link />;
    } else if (key === "tokenAmount") {
      buildAttributes(attributes, info.tokenAmount, decimals);
      continue;
    } else if (key === "amount") {
      let amount;
      if (decimals === undefined) {
        labelSuffix = " (raw)";
        amount = new Intl.NumberFormat("en-US").format(value);
      } else {
        amount = new Intl.NumberFormat("en-US", {
          minimumFractionDigits: decimals,
          maximumFractionDigits: decimals,
        }).format(normalizeTokenAmount(value, decimals));
      }
      tag = <>{amount}</>;
    } else {
      tag = <>{value}</>;
    }

    let label = key.charAt(0).toUpperCase() + key.slice(1) + labelSuffix;

    attributes.push(
      <tr key={key}>
        <td>{label}</td>
        <td className="text-lg-right">{tag}</td>
      </tr>
    );
  }
}
