import React from "react";

export default function ErrorCard({
  retry,
  retryText,
  text
}: {
  retry?: () => void;
  retryText?: string;
  text: string;
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
              <hr></hr>
              <span className="btn btn-white" onClick={retry}>
                {buttonText}
              </span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
