import React from "react";

export default function ErrorCard({
  retry,
  text
}: {
  retry?: () => void;
  text: string;
}) {
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
              Try Again
            </span>
            <div className="d-block d-md-none mt-4">
              <hr></hr>
              <span className="btn btn-white" onClick={retry}>
                Try Again
              </span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
