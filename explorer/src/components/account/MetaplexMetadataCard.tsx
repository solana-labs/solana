import { NFTData } from "providers/accounts";
import ReactJson from "react-json-view";

export function MetaplexMetadataCard({ nftData }: { nftData: NFTData }) {
  return (
    <>
      <div className="card">
        <div className="card-header">
          <div className="row align-items-center">
            <div className="col">
              <h3 className="card-header-title">Metaplex Metadata</h3>
            </div>
          </div>
        </div>

        <div className="card metadata-json-viewer m-4">
          <ReactJson
            src={nftData.metadata}
            theme={"solarized"}
            style={{ padding: 25 }}
          />
        </div>
      </div>
    </>
  );
}
