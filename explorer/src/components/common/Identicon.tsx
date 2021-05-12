import React, { useEffect, useRef } from "react";

// @ts-ignore
import Jazzicon from "@metamask/jazzicon";
import bs58 from "bs58";
import { PublicKey } from "@solana/web3.js";

export function Identicon(props: {
  address?: string | PublicKey;
  style?: React.CSSProperties;
  className?: string;
}) {
  const { style, className } = props;
  const address =
    typeof props.address === "string"
      ? props.address
      : props.address?.toBase58();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (address && ref.current) {
      ref.current.innerHTML = "";
      ref.current.className = className || "";
      ref.current.appendChild(
        Jazzicon(
          style?.width || 16,
          parseInt(bs58.decode(address).toString("hex").slice(5, 15), 16)
        )
      );
    }
  }, [address, style, className]);

  return <div className="identicon-wrapper" ref={ref} style={props.style} />;
}
