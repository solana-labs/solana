import React from "react";
import { Account, useFetchAccountInfo } from "providers/accounts";
import { create } from "superstruct";
import { TableCardBody } from "components/common/TableCardBody";
import { Address } from "components/common/Address";
import { UnknownAccountCard } from "./UnknownAccountCard";
import { Cluster, useCluster } from "providers/cluster";
import { reportError } from "utils/sentry";
import {GatewayTokenAccount, GatewayTokenAccountInfo} from "../../validators/accounts/gateway";
import {InfoTooltip} from "../common/InfoTooltip";

export function GatewayTokenAccountSection({
                                             account,
                                             tokenAccount,
                                           }: {
  account: Account;
  tokenAccount: GatewayTokenAccount;
}) {
  const { cluster } = useCluster();

  try {
    console.log("GT: ", tokenAccount);
    const info = create(tokenAccount.info, GatewayTokenAccountInfo);
    return <GatewayTokenAccountCard account={account} info={info} />;
  } catch (err) {
    if (cluster !== Cluster.Custom) {
      reportError(err, {
        address: account.pubkey.toBase58(),
      });
    }
  }
  return <UnknownAccountCard account={account} />;
}

const timestampToDate = (timestamp: number) => new Date(timestamp * 1000);

const expired = (info: GatewayTokenAccountInfo) => info.expiryTime && timestampToDate(info.expiryTime) < new Date();

function GatewayTokenAccountCard({
                                   account,
                                   info,
                                 }: {
  account: Account;
  info: GatewayTokenAccountInfo;
}) {
  const fetchInfo = useFetchAccountInfo();
  const refresh = () => fetchInfo(account.pubkey);

  return (
    <>
      <div className="card">
        <div className="card-header">
          <h3 className="card-header-title mb-0 d-flex align-items-center">
            Overview
          </h3>
          <button className="btn btn-white btn-sm" onClick={refresh}>
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
          {info.owner && (
            <tr>
              <td>Owner</td>
              <td className="text-lg-right">
                <Address pubkey={info.owner} alignRight link />
              </td>
            </tr>
          )}
          {info.issuingGatekeeper && (
            <tr>
              <td>Issuing Gatekeeper</td>
              <td className="text-lg-right">
                <Address pubkey={info.issuingGatekeeper} alignRight link />
              </td>
            </tr>
          )}
          {info.gatekeeperNetwork && (
            <tr>
              <td>Gatekeeper Network</td>
              <td className="text-lg-right">
                <Address pubkey={info.gatekeeperNetwork} alignRight link />
              </td>
            </tr>
          )}
          {info.state && (
            <tr>
              <td>State</td>
              <td className="text-lg-right">{info.state}</td>
            </tr>
          )}
          {info.expiryTime && (
            <tr>
              <td>Expires</td>
              
                <td className="text-lg-right">
                  <InfoTooltip
                    right
                    text={timestampToDate(info.expiryTime).toISOString()}
                  >
                  {timestampToDate(info.expiryTime).toLocaleString()}
                  {expired(info) && ' EXPIRED'}
                  </InfoTooltip>
                </td>
            </tr>
          )}
        </TableCardBody>
      </div>
    </>
  );
}