import React, { useState, ReactNode } from "react";

type CopyableProps = {
  text: string;
  children: ReactNode;
  bottom?: boolean;
  right?: boolean;
};

type State = "hide" | "copy" | "copied";

function Popover({ state, bottom, right }: { state: State; bottom?: boolean, right?: boolean }) {
  if (state === "hide") return null;
  const text = state === "copy" ? "Copy" : "Copied!";
  return (
    <div className={`popover bs-popover-${bottom ? "bottom" : "top"}${right ? " right" : ""} show`}>
      <div className={`arrow${right ? " right" : ""}`} />
      <div className="popover-body">{text}</div>
    </div>
  );
}

function Copyable({ bottom, right, text, children }: CopyableProps) {
  const [state, setState] = useState<State>("hide");

  const copyToClipboard = () => navigator.clipboard.writeText(text);
  const handleClick = () =>
    copyToClipboard().then(() => {
      setState("copied");
      setTimeout(() => setState("hide"), 1000);
    });

  return (
    <div
      className="popover-container c-pointer"
      onClick={handleClick}
      onMouseOver={() => setState("copy")}
      onMouseOut={() => state === "copy" && setState("hide")}
    >
      {children}
      <Popover bottom={bottom} right={right} state={state} />
    </div>
  );
}

export default Copyable;
