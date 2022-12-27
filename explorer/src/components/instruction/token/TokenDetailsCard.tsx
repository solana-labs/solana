import React from "react";
import { create } from "superstruct";
import {
  SignatureResult,
  ParsedTransaction,
  PublicKey,
  ParsedInstruction,
} from "@solana/web3.js";

import { UnknownDetailsCard } from "../UnknownDetailsCard";
import { InstructionCard } from "../InstructionCard";
import { Address } from "components/common/Address";
import {
  IX_STRUCTS,
  TokenInstructionType,
  IX_TITLES,
  TokenAmountUi,
} from "./types";
import { ParsedInfo } from "validators";
import {
  useTokenAccountInfo,
  useMintAccountInfo,
  useFetchAccountInfo,
} from "providers/accounts";
import { normalizeTokenAmount } from "utils";
import { reportError } from "utils/sentry";
import { useTokenRegistry } from "providers/mints/token-registry";

type DetailsProps = {
  tx: ParsedTransaction;
  ix: ParsedInstruction;
  result: SignatureResult;
  index: number;
  innerCards?: JSX.Element[];
  childIndex?: number;
};

export function TokenDetailsCard(props: DetailsProps) {
  try {
    const parsed = create(props.ix.parsed, ParsedInfo);
    const { type: rawType, info } = parsed;
    const type = create(rawType, TokenInstructionType);
    const title = `Token Program: ${IX_TITLES[type]}`;
    const created = create(info, IX_STRUCTS[type] as any);
    return <TokenInstruction title={title} info={created} {...props} />;
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
  innerCards?: JSX.Element[];
  childIndex?: number;
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
  const { tokenRegistry } = useTokenRegistry();
  const fetchAccountInfo = useFetchAccountInfo();

  React.useEffect(() => {
    if (tokenAddress && !tokenInfo) {
      fetchAccountInfo(new PublicKey(tokenAddress), "parsed");
    }
  }, [fetchAccountInfo, tokenAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  React.useEffect(() => {
    if (mintAddress && !mintInfo) {
      fetchAccountInfo(new PublicKey(mintAddress), "parsed");
    }
  }, [fetchAccountInfo, mintAddress]); // eslint-disable-line react-hooks/exhaustive-deps

  const attributes: JSX.Element[] = [];
  let decimals = mintInfo?.decimals;
  let tokenSymbol = "";

  if ("tokenAmount" in props.info) {
    decimals = props.info.tokenAmount.decimals;
  }

  if (mintAddress) {
    const tokenDetails = tokenRegistry.get(mintAddress);

    if (tokenDetails) {
      tokenSymbol = tokenDetails.symbol;
    }

    attributes.push(
      <tr key={mintAddress}>
        <td>Token</td>
        <td className="text-lg-end">
          <Address pubkey={new PublicKey(mintAddress)} alignRight link />
        </td>
      </tr>
    );
  }

  for (let key in props.info) {
    let value = props.info[key];
    if (value === undefined) continue;

    // Flatten lists of public keys
    if (Array.isArray(value) && value.every((v) => v instanceof PublicKey)) {
      for (let i = 0; i < value.length; i++) {
        let publicKey = value[i];
        let label = `${key.charAt(0).toUpperCase() + key.slice(1)} - #${i + 1}`;

        attributes.push(
          <tr key={key + i}>
            <td>{label}</td>
            <td className="text-lg-end">
              <Address pubkey={publicKey} alignRight link />
            </td>
          </tr>
        );
      }
      continue;
    }

    if (key === "tokenAmount") {
      key = "amount";
      value = (value as TokenAmountUi).amount;
    }

    let tag;
    let labelSuffix = "";
    if (value instanceof PublicKey) {
      tag = <Address pubkey={value} alignRight link />;
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
      tag = (
        <>
          {amount} {tokenSymbol}
        </>
      );
    } else {
      tag = <>{value}</>;
    }

    let label = key.charAt(0).toUpperCase() + key.slice(1) + labelSuffix;

    attributes.push(
      <tr key={key}>
        <td>{label}</td>
        <td className="text-lg-end">{tag}</td>
      </tr>
    );
  }

  return (
    <InstructionCard
      ix={props.ix}
      index={props.index}
      result={props.result}
      title={props.title}
      innerCards={props.innerCards}
      childIndex={props.childIndex}
    >
      {attributes}
    </InstructionCard>
  );
}
