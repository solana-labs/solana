import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { lamportsToSolString } from "utils";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { Address } from "components/common/Address";
import {
  ProgramAccountInfo,
  ProgramBufferAccountInfo,
  ProgramDataAccountInfo,
  UpgradeableLoaderAccount,
} from "validators/accounts/upgradeable-program";
import { Slot } from "components/common/Slot";
import { addressLabel } from "utils/tx";
import { useCluster } from "providers/cluster";
import { ErrorCard } from "components/common/ErrorCard";
import { Copyable } from "components/common/Copyable";

export function UpgradeableLoaderAccountSection({
  account,
  parsedData,
  programData,
}: {
  account: Account;
  parsedData: UpgradeableLoaderAccount;
  programData: ProgramDataAccountInfo | undefined;
}) {
  switch (parsedData.type) {
    case "program": {
      if (programData === undefined) {
        return <ErrorCard text="Invalid Upgradeable Program account" />;
      }
      return (
        <UpgradeableProgramSection
          account={account}
          programAccount={parsedData.info}
          programData={programData}
        />
      );
    }
    case "programData": {
      return (
        <UpgradeableProgramDataSection
          account={account}
          programData={parsedData.info}
        />
      );
    }
    case "buffer": {
      return (
        <UpgradeableProgramBufferSection
          account={account}
          programBuffer={parsedData.info}
        />
      );
    }
  }
}

export function UpgradeableProgramSection({
  account,
  programAccount,
  programData,
}: {
  account: Account;
  programAccount: ProgramAccountInfo;
  programData: ProgramDataAccountInfo;
}) {
  const refresh = useFetchAccountInfo();
  const { cluster } = useCluster();
  const label = addressLabel(account.pubkey.toBase58(), cluster);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Program Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey)}
        >
          <span className="fe fe-refresh-cw mr-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-right">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        {label && (
          <tr>
            <td>Address Label</td>
            <td className="text-lg-right">{label}</td>
          </tr>
        )}
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-right text-uppercase">
            {lamportsToSolString(account.lamports || 0)}
          </td>
        </tr>
        <tr>
          <td>Executable</td>
          <td className="text-lg-right">Yes</td>
        </tr>
        <tr>
          <td>Executable Data</td>
          <td className="text-lg-right">
            <Address pubkey={programAccount.programData} alignRight link />
          </td>
        </tr>
        <tr>
          <td>Upgradeable</td>
          <td className="text-lg-right">
            {programData.authority !== null ? "Yes" : "No"}
          </td>
        </tr>
        <tr>
          <td>Last Deployed Slot</td>
          <td className="text-lg-right">
            <Slot slot={programData.slot} link />
          </td>
        </tr>
        {programData.authority !== null && (
          <tr>
            <td>Upgrade Authority</td>
            <td className="text-lg-right">
              <Address pubkey={programData.authority} alignRight link />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
  );
}

export function UpgradeableProgramDataSection({
  account,
  programData,
}: {
  account: Account;
  programData: ProgramDataAccountInfo;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Program Executable Data Account
          </h3>
          <button
            className="btn btn-white btn-sm"
            onClick={() => refresh(account.pubkey)}
          >
            <span className="fe fe-refresh-cw mr-2"></span>
            Refresh
          </button>
        </div>

        <TableCardBody>
          <tr>
            <td>Address</td>
            <td className="text-lg-right">
              <Address pubkey={account.pubkey} alignRight raw />
            </td>
          </tr>
          <tr>
            <td>Balance (SOL)</td>
            <td className="text-lg-right text-uppercase">
              {lamportsToSolString(account.lamports || 0)}
            </td>
          </tr>
          {account.details?.space !== undefined && (
            <tr>
              <td>Data (Bytes)</td>
              <td className="text-lg-right">{account.details.space}</td>
            </tr>
          )}
          <tr>
            <td>Upgradeable</td>
            <td className="text-lg-right">
              {programData.authority !== null ? "Yes" : "No"}
            </td>
          </tr>
          <tr>
            <td>Last Deployed Slot</td>
            <td className="text-lg-right">
              <Slot slot={programData.slot} link />
            </td>
          </tr>
          {programData.authority !== null && (
            <tr>
              <td>Upgrade Authority</td>
              <td className="text-lg-right">
                <Address pubkey={programData.authority} alignRight link />
              </td>
            </tr>
          )}
        </TableCardBody>
      </div>
      <DataContainer
        title="Program Data"
        data={programData.data[0]}
        type={programData.data[1]}
      />
    </>
  );
}

export function UpgradeableProgramBufferSection({
  account,
  programBuffer,
}: {
  account: Account;
  programBuffer: ProgramBufferAccountInfo;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Program Deploy Buffer Account
          </h3>
          <button
            className="btn btn-white btn-sm"
            onClick={() => refresh(account.pubkey)}
          >
            <span className="fe fe-refresh-cw mr-2"></span>
            Refresh
          </button>
        </div>

        <TableCardBody>
          <tr>
            <td>Address</td>
            <td className="text-lg-right">
              <Address pubkey={account.pubkey} alignRight raw />
            </td>
          </tr>
          <tr>
            <td>Balance (SOL)</td>
            <td className="text-lg-right text-uppercase">
              {lamportsToSolString(account.lamports || 0)}
            </td>
          </tr>
          {account.details?.space !== undefined && (
            <tr>
              <td>Data (Bytes)</td>
              <td className="text-lg-right">{account.details.space}</td>
            </tr>
          )}
          {programBuffer.authority !== null && (
            <tr>
              <td>Deploy Authority</td>
              <td className="text-lg-right">
                <Address pubkey={programBuffer.authority} alignRight link />
              </td>
            </tr>
          )}
          {account.details && (
            <tr>
              <td>Owner</td>
              <td className="text-lg-right">
                <Address pubkey={account.details.owner} alignRight link />
              </td>
            </tr>
          )}
        </TableCardBody>
      </div>
      <DataContainer
        title="Buffer Data"
        data={programBuffer.data[0]}
        type={programBuffer.data[1]}
      />
    </>
  );
}

function DataContainer({
  title,
  data,
  type,
}: {
  title: string;
  data: string;
  type: string;
}) {
  return (
    <>
      <div className="container">
        <div className="header">
          <div className="header-body">
            <Copyable text={data}>
              <h3
                className="card-header-title"
                style={{ display: "inline-block" }}
              >
                {title} <small className="text-muted">({type})</small>
              </h3>
            </Copyable>
          </div>
        </div>
      </div>
      <div className="card">
        <div className="data-view">{data}</div>
      </div>
    </>
  );
}
