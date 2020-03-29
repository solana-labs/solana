import React, { useState } from "react";
import { Link } from "gatsby-plugin-intl";
import { Transition } from "react-transition-group";
import { Navbar, Dropdown } from "react-bootstrap";

import SolanaDropdownMenu from "./dropdown-menu";
import SolanaDropdownToggle from "./dropdown-toggle";
import SolanaLogo from "../img/logos-solana/dark-horizontal-combined-green.png";
import Fire from "../img/icons/Fire.inline.svg";
import Bulb from "../img/icons/Bulb.inline.svg";
import Chat from "../img/icons/Chat.inline.svg";
import Clipboard from "../img/icons/Clipboard.inline.svg";

const Header = () => {
  const [showDocumentation, updateShowDocumentation] = useState(false);

  return (
    <Navbar expand="lg" fixed="top" className="navbar-dark bg-black">
      <div className="container-fluid">
        <Link to="/">
          <img src={SolanaLogo} className="navbar-brand-img bg-black p-1 btn" />
        </Link>

        <Navbar.Toggle aria-controls="navbarCollapse" />
        <Navbar.Collapse id="navbarCollapse">
          <Navbar.Toggle aria-controls="navbarCollapse">
            <i className="fe fe-x"></i>
          </Navbar.Toggle>

          <ul className="navbar-nav ml-auto">
            <Transition in={showDocumentation} timeout={1}>
              {state => {
                const show = state == "entered" || state == "exiting";
                const update = updateShowDocumentation;
                return (
                  <Dropdown
                    as="li"
                    className={`nav-item hovered bg-black`}
                    onMouseEnter={() => update(true)}
                    onMouseLeave={() => update(false)}
                    onToggle={() => update(!show)}
                    show={show}
                  >
                    <Dropdown.Toggle
                      as={SolanaDropdownToggle}
                      to="/developers"
                      id="navbarDocumentation"
                      expanded={show}
                    >
                      Developers
                    </Dropdown.Toggle>

                    <Dropdown.Menu
                      as={SolanaDropdownMenu}
                      labeledBy="navbarDocumentation"
                    >
                      <a
                        className="list-group-item bg-black"
                        href="https://docs.solana.com"
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        {/* <!-- Icon --> */}
                        <div className="icon icon-sm text-success">
                          <Clipboard />
                        </div>

                        {/* <!-- Content --> */}
                        <div className="ml-4">
                          {/* <!-- Heading --> */}
                          <h6 className="font-weight-bold text-uppercase text-success mb-0">
                            Documentation
                          </h6>

                          {/* <!-- Text --> */}
                          <p className="font-size-sm text-gray-700 mb-0">
                            Jump right in
                          </p>
                        </div>

                        {/* <!-- Badge --> */}
                        <span className="badge badge-pill badge-success-soft ml-auto">
                          1.0.0
                        </span>
                      </a>

                      <Link
                        className="list-group-item bg-black"
                        to="/developers"
                      >
                        {/* <!-- Icon --> */}
                        <div className="icon icon-sm text-success">
                          <Bulb />
                        </div>

                        {/* <!-- Content --> */}
                        <div className="ml-4">
                          {/* <!-- Heading --> */}
                          <h6 className="font-weight-bold text-uppercase text-success mb-0">
                            Learn More
                          </h6>

                          {/* <!-- Text --> */}
                          <p className="font-size-sm text-gray-700 mb-0">
                            &ldquo;Why Solana?&rdquo; Come find out.
                          </p>
                        </div>
                      </Link>

                      <Link
                        className="list-group-item bg-black"
                        to="/accelerator"
                      >
                        {/* <!-- Icon --> */}
                        <div className="icon icon-sm text-success">
                          <Fire />
                        </div>

                        {/* <!-- Content --> */}
                        <div className="ml-4">
                          {/* <!-- Heading --> */}
                          <h6 className="font-weight-bold text-uppercase text-success mb-0">
                            Accelerator
                          </h6>

                          {/* <!-- Text --> */}
                          <p className="font-size-sm text-gray-700 mb-0">
                            Apply for funding
                          </p>
                        </div>
                      </Link>

                      <a
                        className="list-group-item bg-black"
                        href="https://discordapp.com/invite/pquxPsq"
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        {/* <!-- Icon --> */}
                        <div className="icon icon-sm text-success">
                          <Chat />
                        </div>

                        {/* <!-- Content --> */}
                        <div className="ml-4">
                          {/* <!-- Heading --> */}
                          <h6 className="font-weight-bold text-uppercase text-success mb-0">
                            Chat
                          </h6>

                          {/* <!-- Text --> */}
                          <p className="font-size-sm text-gray-700 mb-0">
                            Live support from Solana team
                          </p>
                        </div>
                      </a>
                    </Dropdown.Menu>
                  </Dropdown>
                );
              }}
            </Transition>

            <li className="nav-item">
              <Link className="nav-link" activeClassName="active" to="/tokens">
                Tokens
              </Link>
            </li>

            <li className="nav-item">
              <Link
                className="nav-link"
                activeClassName="active"
                to="/validators"
              >
                Validators
              </Link>
            </li>

            <li className="nav-item">
              <Link
                className="nav-link"
                activeClassName="active"
                to="/community"
              >
                Community
              </Link>
            </li>

            <li className="nav-item">
              <Link className="nav-link" activeClassName="active" to="/about">
                About
              </Link>
            </li>
          </ul>

          {/* <!-- Button --> */}
          <div id="header-right-button" className="ml-auto">
            <a
              className="navbar-btn btn btn-sm btn-outline-warning lift"
              href="https://discordapp.com/invite/pquxPsq"
              target="_blank"
              rel="noopener noreferrer"
            >
              Chat<i className="fe fe-arrow-right ml-3"></i>
            </a>
          </div>
        </Navbar.Collapse>
      </div>
    </Navbar>
  );
};

export default Header;
