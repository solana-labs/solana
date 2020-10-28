import React, { useState } from "react";
import { Link } from "react-router-dom";

type CopyState = "copy" | "copied";
type Props = {
  slot: number;
  link?: boolean;
};
export function Slot({ slot, link }: Props) {
  const [state, setState] = useState<CopyState>("copy");

  const copyToClipboard = () => navigator.clipboard.writeText(slot.toString());
  const handleClick = () =>
    copyToClipboard().then(() => {
      setState("copied");
      setTimeout(() => setState("copy"), 1000);
    });

  const copyIcon =
    state === "copy" ? (
      <span className="fe fe-copy" onClick={handleClick}></span>
    ) : (
      <span className="fe fe-check-circle"></span>
    );

  const copyButton = (
    <span className="c-pointer font-size-tiny mr-2">{copyIcon}</span>
  );

  return link ? (
    <span className="text-monospace">
      {copyButton}
      <Link className="" to={`/block/${slot}`}>
        {slot.toLocaleString("en-US")}
      </Link>
    </span>
  ) : (
    <span className="text-monospace">{slot.toLocaleString("en-US")}</span>
  );
}
