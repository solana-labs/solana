import React from "react";
import Image from "next/image";
import Link from "next/link";
import { useRouter } from "next/router";
import { clusterPath } from "src/utils/url";
import { ClusterStatusButton } from "src/components/ClusterStatusButton";
import { NavLink } from "src/components/NavLink";

export function Navbar() {
  const router = useRouter();

  // TODO: use `collapsing` to animate collapsible navbar
  const [collapse, setCollapse] = React.useState(false);

  return (
    <nav className="navbar navbar-expand-md navbar-light">
      <div className="container">
        <Link href={clusterPath("/", router.asPath)} passHref>
          <a>
            <Image src="/img/logos-solana/dark-explorer-logo.svg" width={250} height={21.48} alt="Solana Explorer" />
          </a>
        </Link>

        <button
          className="navbar-toggler"
          type="button"
          onClick={() => setCollapse((value) => !value)}
        >
          <span className="navbar-toggler-icon"></span>
        </button>

        <div
          className={`collapse navbar-collapse ms-auto me-4 ${
            collapse ? "show" : ""
          }`}
        >
          <ul className="navbar-nav me-auto">
            <li className="nav-item">
              <NavLink activeClassName="active" href={clusterPath("/", router.asPath)}>
                <span className="nav-link">
                  Cluster Stats
                </span>
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink activeClassName="active" href={clusterPath("/supply", router.asPath)}>
                <span className="nav-link">
                  Supply
                </span>
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink activeClassName="active" href={clusterPath("/tx/inspector", router.asPath)}>
                <span className="nav-link">
                  Inspector
                </span>
              </NavLink>
            </li>
          </ul>
        </div>

        <div className="d-none d-md-block">
          <ClusterStatusButton />
        </div>
      </div>
    </nav>
  );
}
