import React from "react";
import { Link } from "react-router-dom";
import { useClusterModal } from "providers/cluster";
import ClusterStatusButton from "components/ClusterStatusButton";
import { pickCluster } from "utils/url";

export type Tab = "Transactions" | "Accounts" | "Supply";

type Props = { children: React.ReactNode; tab: Tab };
export default function TabbedPage({ children, tab }: Props) {
  const [, setShow] = useClusterModal();

  return (
    <div className="container">
      <div className="header">
        <div className="header-body">
          <div className="row align-items-center d-md-none">
            <div className="col-12">
              <ClusterStatusButton expand onClick={() => setShow(true)} />
            </div>
          </div>
          <div className="row align-items-center">
            <div className="col">
              <ul className="nav nav-tabs nav-overflow header-tabs">
                <li className="nav-item">
                  <NavLink
                    href="/transactions"
                    tab="Transactions"
                    current={tab}
                  />
                </li>
                <li className="nav-item">
                  <NavLink href="/accounts" tab="Accounts" current={tab} />
                </li>
                <li className="nav-item">
                  <NavLink href="/supply" tab="Supply" current={tab} />
                </li>
              </ul>
            </div>
            <div className="col-auto d-none d-md-block">
              <ClusterStatusButton onClick={() => setShow(true)} />
            </div>
          </div>
        </div>
      </div>

      {children}
    </div>
  );
}

function NavLink({
  href,
  tab,
  current
}: {
  href: string;
  tab: Tab;
  current: Tab;
}) {
  let classes = "nav-link";
  if (tab === current) {
    classes += " active";
  }

  return (
    <Link
      to={location => ({ ...pickCluster(location), pathname: href })}
      className={classes}
    >
      {tab}
    </Link>
  );
}
