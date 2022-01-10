import React, { ReactNode, useState } from "react";

type CopyState = "copy" | "copied" | "errored";

export function Copyable({
  text,
  children,
  replaceText,
}: {
  text: string;
  children: ReactNode;
  replaceText?: boolean;
}) {
  const [state, setState] = useState<CopyState>("copy");

  const handleClick = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch (err) {
      setState("errored");
    }
    setTimeout(() => setState("copy"), 1000);
  };

  function CopyIcon() {
    if (state === "copy") {
      return (
        <span className="fe fe-copy c-pointer" onClick={handleClick}></span>
      );
    } else if (state === "copied") {
      return <span className="fe fe-check-circle"></span>;
    } else if (state === "errored") {
      return (
        <span
          className="fe fe-x-circle"
          title="Please check your browser's copy permissions."
        ></span>
      );
    }
    return null;
  }

  let message: string | undefined;
  let textColor = "";
  if (state === "copied") {
    message = "Copied";
    textColor = "text-info";
  } else if (state === "errored") {
    message = "Copy Failed";
    textColor = "text-danger";
  }

  function PrependCopyIcon() {
    return (
      <>
        <span className="font-size-tiny me-2">
          <span className={textColor}>
            {message !== undefined && <span className="me-2">{message}</span>}
            <CopyIcon />
          </span>
        </span>
        {children}
      </>
    );
  }

  function ReplaceWithMessage() {
    return (
      <span className="d-flex flex-column flex-nowrap">
        <span className="font-size-tiny">
          <span className={textColor}>
            <CopyIcon />
            <span className="ms-2">{message}</span>
          </span>
        </span>
        <span className="v-hidden">{children}</span>
      </span>
    );
  }

  if (state === "copy") {
    return <PrependCopyIcon />;
  } else if (replaceText) {
    return <ReplaceWithMessage />;
  }

  return (
    <>
      <span className="d-none d-lg-inline">
        <PrependCopyIcon />
      </span>
      <span className="d-inline d-lg-none">
        <ReplaceWithMessage />
      </span>
    </>
  );
}
