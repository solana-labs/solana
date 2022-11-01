import React from "react";
import { TableCardBody } from "components/common/TableCardBody";
import { SolBalance } from "components/common/SolBalance";
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
import { UnknownAccountCard } from "components/account/UnknownAccountCard";
import { Downloadable } from "components/common/Downloadable";
import { CheckingBadge, VerifiedBadge } from "components/common/VerifiedBadge";
import { InfoTooltip } from "components/common/InfoTooltip";
import { useVerifiableBuilds } from "utils/program-verification";
import { SecurityTXTBadge } from "components/common/SecurityTXTBadge";

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
    case "uninitialized": {
      return <UnknownAccountCard account={account} />;
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
  programData: ProgramDataAccountInfo | undefined;
}) {
  const refresh = useFetchAccountInfo();
  const { cluster } = useCluster();
  const label = addressLabel(account.pubkey.toBase58(), cluster);
  const { loading, verifiableBuilds } = useVerifiableBuilds(account.pubkey);
  return (
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          {programData === undefined && "Closed "}Program Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey, "parsed")}
        >
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-end">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        {label && (
          <tr>
            <td>Address Label</td>
            <td className="text-lg-end">{label}</td>
          </tr>
        )}
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-end text-uppercase">
            <SolBalance lamports={account.lamports} />
          </td>
        </tr>
        <tr>
          <td>Executable</td>
          <td className="text-lg-end">
            {programData !== undefined ? "Yes" : "No"}
          </td>
        </tr>
        <tr>
          <td>Executable Data{programData === undefined && " (Closed)"}</td>
          <td className="text-lg-end">
            <Address pubkey={programAccount.programData} alignRight link />
          </td>
        </tr>
        {programData !== undefined && (
          <>
            <tr>
              <td>Upgradeable</td>
              <td className="text-lg-end">
                {programData.authority !== null ? "Yes" : "No"}
              </td>
            </tr>
            <tr>
              <td>
                <LastVerifiedBuildLabel />
              </td>
              <td className="text-lg-end">
                {loading ? (
                  <CheckingBadge />
                ) : (
                  <>
                    {verifiableBuilds.map((b, i) => (
                      <VerifiedBadge
                        key={i}
                        verifiableBuild={b}
                        deploySlot={programData.slot}
                      />
                    ))}
                  </>
                )}
              </td>
            </tr>
            <tr>
              <td>
                <SecurityLabel />
              </td>
              <td className="text-lg-end">
                <SecurityTXTBadge
                  programData={programData}
                  pubkey={account.pubkey}
                />
              </td>
            </tr>
            <tr>
              <td>Last Deployed Slot</td>
              <td className="text-lg-end">
                <Slot slot={programData.slot} link />
              </td>
            </tr>
            {programData.authority !== null && (
              <tr>
                <td>Upgrade Authority</td>
                <td className="text-lg-end">
                  <Address pubkey={programData.authority} alignRight link />
                </td>
              </tr>
            )}
          </>
        )}
      </TableCardBody>
    </div>
  );
}

function SecurityLabel() {
  return (
    <InfoTooltip text="Security.txt helps security researchers to contact developers if they find security bugs.">
      <a
        rel="noopener noreferrer"
        target="_blank"
        href="https://github.com/neodyme-labs/solana-security-txt"
      >
        <span className="security-txt-link-color-hack-reee">Security.txt</span>
        <span className="fe fe-external-link ms-2"></span>
      </a>
    </InfoTooltip>
  );
}

function LastVerifiedBuildLabel() {
  return (
    <InfoTooltip text="Indicates whether the program currently deployed on-chain is verified to match the associated published source code, when it is available.">
      Verifiable Build Status (experimental)
    </InfoTooltip>
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
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Program Executable Data Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey, "parsed")}
        >
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-end">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-end text-uppercase">
            <SolBalance lamports={account.lamports} />
          </td>
        </tr>
        {account.space !== undefined && (
          <tr>
            <td>Data Size (Bytes)</td>
            <td className="text-lg-end">
              <Downloadable
                data={programData.data[0]}
                filename={`${account.pubkey.toString()}.bin`}
              >
                <span className="me-2">{account.space}</span>
              </Downloadable>
            </td>
          </tr>
        )}
        <tr>
          <td>Upgradeable</td>
          <td className="text-lg-end">
            {programData.authority !== null ? "Yes" : "No"}
          </td>
        </tr>
        <tr>
          <td>Last Deployed Slot</td>
          <td className="text-lg-end">
            <Slot slot={programData.slot} link />
          </td>
        </tr>
        {programData.authority !== null && (
          <tr>
            <td>Upgrade Authority</td>
            <td className="text-lg-end">
              <Address pubkey={programData.authority} alignRight link />
            </td>
          </tr>
        )}
      </TableCardBody>
    </div>
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
    <div className="card">
      <div className="card-header">
        <h3 className="card-header-title mb-0 d-flex align-items-center">
          Program Deploy Buffer Account
        </h3>
        <button
          className="btn btn-white btn-sm"
          onClick={() => refresh(account.pubkey, "parsed")}
        >
          <span className="fe fe-refresh-cw me-2"></span>
          Refresh
        </button>
      </div>

      <TableCardBody>
        <tr>
          <td>Address</td>
          <td className="text-lg-end">
            <Address pubkey={account.pubkey} alignRight raw />
          </td>
        </tr>
        <tr>
          <td>Balance (SOL)</td>
          <td className="text-lg-end text-uppercase">
            <SolBalance lamports={account.lamports} />
          </td>
        </tr>
        {account.space !== undefined && (
          <tr>
            <td>Data Size (Bytes)</td>
            <td className="text-lg-end">{account.space}</td>
          </tr>
        )}
        {programBuffer.authority !== null && (
          <tr>
            <td>Deploy Authority</td>
            <td className="text-lg-end">
              <Address pubkey={programBuffer.authority} alignRight link />
            </td>
          </tr>
        )}
        <tr>
          <td>Owner</td>
          <td className="text-lg-end">
            <Address pubkey={account.owner} alignRight link />
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}
