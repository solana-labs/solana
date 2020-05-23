import React from "react";

export default function ErrorCard({
  retry,
  retryText,
  text,
  subtext
}: {
  retry?: () => void;
  retryText?: string;
  text: string;
  subtext?: string;
}) {
  const buttonText = retryText || "Try Again";
  return (
    <div className="card">
      <div className="card-body text-center">
        {text}
        {retry && (
          <>
            <span
              className="btn btn-white ml-3 d-none d-md-inline"
              onClick={retry}
            >
              {buttonText}
            </span>
            <div className="d-block d-md-none mt-4">
              <span className="btn btn-white w-100" onClick={retry}>
                {buttonText}
              </span>
            </div>
            {subtext && (
              <div className="text-muted">
                <hr></hr>
                {subtext}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
