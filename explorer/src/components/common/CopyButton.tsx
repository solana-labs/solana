import React, { useState } from "react";

type CopyState = "copy" | "copied" | "errored";

export function CopyButton({ text }: { text: string }) {
  const [state, setState] = useState<CopyState>("copy");

  const copyToClipboard = () => navigator.clipboard.writeText(text);
  const handleClick = () =>
    copyToClipboard()
      .then(() => {
        setState("copied");
        setTimeout(() => setState("copy"), 1000);
      })
      .catch((error) => {
        setState("errored");
      });

  let copyIcon = <span className="fe fe-copy" onClick={handleClick}></span>;

  if (state === "errored") {
    copyIcon = (
      <span
        className="fe fe-x-circle text-danger"
        title="Please check your browser's copy permissions."
      ></span>
    );
  } else if (state === "copied") {
    copyIcon = <span className="fe fe-check-circle"></span>;
  }

  return (
    <span
      className={`${
        state !== "errored" ? "c-pointer" : ""
      } font-size-tiny mr-2`}
    >
      {copyIcon}
    </span>
  );
}
