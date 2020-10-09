import React, { useState } from "react";
import { Link } from "react-router-dom";
import { clusterPath } from "utils/url";

type CopyState = "copy" | "copied";
type Props = {
  block: string;
  alignRight?: boolean;
  link?: boolean;
};

export function Block({ block, alignRight, link }: Props) {
  const [state, setState] = useState<CopyState>("copy");

  const copyToClipboard = () => navigator.clipboard.writeText(block);
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

  const content = (
    <>
      <span className="c-pointer font-size-tiny mr-2">{copyIcon}</span>
      <span className="text-monospace">
        {link ? (
          <Link to={clusterPath(`/block/${block}`)}>{block}</Link>
        ) : (
          <span>{block}</span>
        )}
      </span>
    </>
  );

  return (
    <>
      <div
        className={`d-none d-lg-flex align-items-center ${
          alignRight ? "justify-content-end" : ""
        }`}
      >
        {content}
      </div>
      <div className="d-flex d-lg-none align-items-center">{content}</div>
    </>
  );
}
