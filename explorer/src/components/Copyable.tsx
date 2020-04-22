import React, { useState, ReactNode } from "react";

type CopyableProps = {
  text: string;
  children: ReactNode;
};

type State = "hide" | "copy" | "copied";

function Popover({ state }: { state: State }) {
  if (state === "hide") return null;
  const text = state === "copy" ? "Copy" : "Copied!";
  return (
    <div className="popover bs-popover-top show">
      <div className="arrow" />
      <div className="popover-body">{text}</div>
    </div>
  );
}

function Copyable({ text, children }: CopyableProps) {
  const [state, setState] = useState<State>("hide");

  const copyToClipboard = () => navigator.clipboard.writeText(text);
  const handleClick = () =>
    copyToClipboard().then(() => {
      setState("copied");
      setTimeout(() => setState("hide"), 1000);
    });

  return (
    <div className="copyable">
      <div
        onClick={handleClick}
        onMouseOver={() => setState("copy")}
        onMouseOut={() => state === "copy" && setState("hide")}
      >
        {children}
      </div>
      <Popover state={state} />
    </div>
  );
}

export default Copyable;
