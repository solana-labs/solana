import React from "react";
import Logo from "img/logos-solana/dark-explorer-logo.svg";
import Link from "next/link";
import { useCreateClusterPath } from "utils/routing";
import { ClusterStatusButton } from "components/ClusterStatusButton";
import { NavLink } from "components/NavLink";

export function Navbar() {
  const createClusterPath = useCreateClusterPath();

  // TODO: use `collapsing` to animate collapsible navbar
  const [collapse, setCollapse] = React.useState(false);

  return (
    <nav className="navbar navbar-expand-md navbar-light">
      <div className="container">
        <Link href={createClusterPath("/")}>
          <a>
            <img src={Logo} width="250" alt="Solana Explorer" />
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
              <NavLink href={createClusterPath("/")}>
                <a className="nav-link">Cluster Stats</a>
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink href={createClusterPath("/supply")}>
                <a className="nav-link">Supply</a>
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink href={createClusterPath("/tx/inspector")}>
                <a className="nav-link">Inspector</a>
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
