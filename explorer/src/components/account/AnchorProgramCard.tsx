import React from "react";
import { Program } from "@project-serum/anchor";
import ReactJson from "react-json-view";

export function AnchorProgramCard({ program }: { program: Program }) {
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Anchor IDL</h3>
            </div>
          </div>
        </div>

        <div className="card metadata-json-viewer m-4">
          <ReactJson
            src={program.idl}
            theme={"solarized"}
            style={{ padding: 25 }}
          />
        </div>
      </div>
    </>
  );
}
