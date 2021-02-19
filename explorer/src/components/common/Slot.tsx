import React, { useState } from "react";
import { Link } from "react-router-dom";
import { clusterPath } from "utils/url";
import { CopyButton } from "./CopyButton";

type CopyState = "copy" | "copied";
type Props = {
  slot: number;
  link?: boolean;
};
export function Slot({ slot, link }: Props) {
  return link ? (
    <span className="text-monospace">
      <CopyButton text={slot.toString()} />
      <Link to={clusterPath(`/block/${slot}`)}>
        {slot.toLocaleString("en-US")}
      </Link>
    </span>
  ) : (
    <span className="text-monospace">{slot.toLocaleString("en-US")}</span>
  );
}
