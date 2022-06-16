import React from "react";
import Logo from "img/logos-solana/dark-explorer-logo.svg";
import { clusterPath } from "utils/url";
import { Link, NavLink } from "react-router-dom";
import { ClusterStatusButton } from "components/ClusterStatusButton";
import { WalletDisconnectButton, WalletMultiButton } from "@solana/wallet-adapter-react-ui";
import { useWallet } from "@solana/wallet-adapter-react";

export function Navbar() {
  // TODO: use `collapsing` to animate collapsible navbar
  const [showSidebar, setShowSidebar] = React.useState(false);
  const [collapse, setCollapse] = React.useState(false);
  // find user address from wallet

  const { connected } = useWallet();

  return (
    <nav className="navbar navbar-expand-md navbar-light">
      <div className="container">
        <Link to={clusterPath("/")}>
          <img src={Logo} width="250" alt="Solana Explorer" />
        </Link>

        <button
          className="navbar-toggler"
          type="button"
          onClick={() => setCollapse((value) => !value)}
        >
          <span className="navbar-toggler-icon"></span>
        </button>

        <div
          className={`collapse navbar-collapse ms-auto me-4 ${collapse ? "show" : ""
            }`}
        >
          <ul className="navbar-nav me-auto">
            <li className="nav-item">
              <NavLink className="nav-link" to={clusterPath("/")} exact>
                Cluster Stats
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink className="nav-link" to={clusterPath("/supply")}>
                Supply
              </NavLink>
            </li>
            <li className="nav-item">
              <NavLink className="nav-link" to={clusterPath("/tx/inspector")}>
                Inspector
              </NavLink>
            </li>
            <li className="nav-item">
              <button className={connected ? 'button-hidden' : 'button'}><span className="fe fe-zap"></span>Connect Wallet</button>
              <WalletMultiButton className={connected ? 'user-address btn btn-primary' : 'connect-btn'} />
            </li>
            <li className="nav-item">
              {connected ? (
                <span className="fe fe-globe btn btn-primary globe" onClick={() => setShowSidebar(true)}></span>
              ) : null}

              {showSidebar ? (
                <div className="sidenav">
                  <div className="sidenav-header">
                    <span className="fe fe-x close" onClick={() => setShowSidebar(false)}></span>
                  </div>
                  <div className="sidenav-body">
                    <div className="sidenav-item">
                      <button>
                        <span className="fe fe-user"></span>Account
                      </button>
                    </div>
                    <div className="sidenav-item">
                      <button><span className="fe fe-repeat"></span>Swap Tokens</button>
                    </div>
                    <hr />
                    <div className="sidenav-item-disconnect">
                      <button><span className="fe fe-zap-off"></span>Disconnect</button>
                      <WalletDisconnectButton className="disconnect-btn" onClick={() => setShowSidebar(false)} />
                    </div>
                  </div>
                </div>)
                : null}

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
