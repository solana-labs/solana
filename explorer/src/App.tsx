import React from "react";
import { Link, Switch, Route, Redirect } from "react-router-dom";

import AccountsCard from "./components/AccountsCard";
import AccountDetails from "./components/AccountDetails";
import TransactionsCard from "./components/TransactionsCard";
import TransactionDetails from "./components/TransactionDetails";
import ClusterModal from "./components/ClusterModal";
import Logo from "./img/logos-solana/light-explorer-logo.svg";
import { TX_ALIASES } from "./providers/transactions";
import { ACCOUNT_ALIASES, ACCOUNT_ALIASES_PLURAL } from "./providers/accounts";
import TabbedPage from "components/TabbedPage";
import TopAccountsCard from "components/TopAccountsCard";
import SupplyCard from "components/SupplyCard";
import StatsCard from "components/StatsCard";
import { pickCluster } from "utils/url";
import Banner from "components/Banner";
import SolanabeachComponents from "./components/SolanabeachComponents";

import { library } from '@fortawesome/fontawesome-svg-core';
import {
  fab,
  faFacebook,
  faTwitter,
  faVuejs,
  faReact,
  faHtml5,
  faGoogle,
  faInstagram,
  faPinterest,
  faYoutube,
  faDiscord,
  faSlack,
  faDribbble,
  faGithub
} from '@fortawesome/free-brands-svg-icons';
import {
  far,
  faSquare,
  faLifeRing,
  faCheckCircle,
  faTimesCircle,
  faDotCircle,
  faThumbsUp,
  faComments,
  faFolderOpen,
  faTrashAlt,
  faFileImage,
  faFileArchive,
  faCommentDots,
  faFolder,
  faKeyboard,
  faCalendarAlt,
  faEnvelope,
  faAddressCard,
  faMap,
  faObjectGroup,
  faImages,
  faUser,
  faLightbulb,
  faGem,
  faClock,
  faUserCircle,
  faQuestionCircle,
  faBuilding,
  faBell,
  faFileExcel,
  faFileAudio,
  faFileVideo,
  faFileWord,
  faFilePdf,
  faFileCode,
  faFileAlt,
  faEye,
  faChartBar
} from '@fortawesome/free-regular-svg-icons';
import {
  fas,
  faAngleDoubleRight,
  faAngleDoubleLeft,
  faSmile,
  faHeart,
  faBatteryEmpty,
  faBatteryFull,
  faChevronRight,
  faSitemap,
  faPrint,
  faMapMarkedAlt,
  faTachometerAlt,
  faAlignCenter,
  faExternalLinkAlt,
  faShareSquare,
  faInfoCircle,
  faSync,
  faQuoteRight,
  faStarHalfAlt,
  faShapes,
  faCarBattery,
  faTable,
  faCubes,
  faPager,
  faCameraRetro,
  faBomb,
  faNetworkWired,
  faBusAlt,
  faBirthdayCake,
  faEyeDropper,
  faUnlockAlt,
  faDownload,
  faAward,
  faPlayCircle,
  faReply,
  faUpload,
  faBars,
  faEllipsisV,
  faSave,
  faSlidersH,
  faCaretRight,
  faChevronUp,
  faPlus,
  faLemon,
  faChevronLeft,
  faTimes,
  faChevronDown,
  faFilm,
  faSearch,
  faEllipsisH,
  faCog,
  faArrowsAltH,
  faPlusCircle,
  faAngleRight,
  faAngleUp,
  faAngleLeft,
  faAngleDown,
  faArrowUp,
  faArrowDown,
  faArrowRight,
  faArrowLeft,
  faStar,
  faSignOutAlt,
  faLink
} from '@fortawesome/free-solid-svg-icons';

library.add(
    far,
    faSquare,
    faLifeRing,
    faCheckCircle,
    faTimesCircle,
    faDotCircle,
    faThumbsUp,
    faComments,
    faFolderOpen,
    faTrashAlt,
    faFileImage,
    faFileArchive,
    faCommentDots,
    faFolder,
    faKeyboard,
    faCalendarAlt,
    faEnvelope,
    faAddressCard,
    faMap,
    faObjectGroup,
    faImages,
    faUser,
    faLightbulb,
    faGem,
    faClock,
    faUserCircle,
    faQuestionCircle,
    faBuilding,
    faBell,
    faFileExcel,
    faFileAudio,
    faFileVideo,
    faFileWord,
    faFilePdf,
    faFileCode,
    faFileAlt,
    faEye,
    faChartBar
);
library.add(
    fab,
    faFacebook,
    faTwitter,
    faVuejs,
    faReact,
    faHtml5,
    faGoogle,
    faInstagram,
    faPinterest,
    faYoutube,
    faDiscord,
    faSlack,
    faDribbble,
    faGithub
);
library.add(
    fas,
    faAngleDoubleRight,
    faAngleDoubleLeft,
    faSmile,
    faHeart,
    faBatteryEmpty,
    faBatteryFull,
    faChevronRight,
    faSitemap,
    faPrint,
    faMapMarkedAlt,
    faTachometerAlt,
    faAlignCenter,
    faExternalLinkAlt,
    faShareSquare,
    faInfoCircle,
    faSync,
    faQuoteRight,
    faStarHalfAlt,
    faShapes,
    faCarBattery,
    faTable,
    faCubes,
    faPager,
    faCameraRetro,
    faBomb,
    faNetworkWired,
    faBusAlt,
    faBirthdayCake,
    faEyeDropper,
    faUnlockAlt,
    faDownload,
    faAward,
    faPlayCircle,
    faReply,
    faUpload,
    faBars,
    faEllipsisV,
    faSave,
    faSlidersH,
    faCaretRight,
    faChevronUp,
    faPlus,
    faLemon,
    faChevronLeft,
    faTimes,
    faChevronDown,
    faFilm,
    faSearch,
    faEllipsisH,
    faCog,
    faArrowsAltH,
    faPlusCircle,
    faAngleRight,
    faAngleUp,
    faAngleLeft,
    faAngleDown,
    faArrowUp,
    faArrowDown,
    faArrowRight,
    faArrowLeft,
    faStar,
    faSignOutAlt,
    faLink
);

function App() {
  return (
    <>
      <ClusterModal />
      <div className="main-content">
        <nav className="navbar navbar-expand-xl navbar-light">
          <div className="container">
            <div className="row align-items-end">
              <div className="col">
                <Link
                  to={(location) => ({
                    ...pickCluster(location),
                    pathname: "/",
                  })}
                >
                  <img src={Logo} width="250" alt="Solana Explorer" />
                </Link>
              </div>
            </div>
          </div>
        </nav>

        <Banner />

        <Switch>
          <Route exact path="/supply">
            <TabbedPage tab="Supply">
              <SupplyCard />
              <TopAccountsCard />
            </TabbedPage>
          </Route>
          <Route
            exact
            path="/accounts/top"
            render={({ location }) => (
              <Redirect to={{ ...location, pathname: "/supply" }} />
            )}
          ></Route>
          <Route
            exact
            path={TX_ALIASES.flatMap((tx) => [tx, tx + "s"]).map(
              (tx) => `/${tx}/:signature`
            )}
            render={({ match }) => (
              <TransactionDetails signature={match.params.signature} />
            )}
          />
          <Route exact path={TX_ALIASES.map((tx) => `/${tx}s`)}>
            <TabbedPage tab="Transactions">
              <TransactionsCard />
            </TabbedPage>
          </Route>
          <Route
            exact
            path={ACCOUNT_ALIASES.concat(ACCOUNT_ALIASES_PLURAL).map(
              (account) => `/${account}/:address`
            )}
            render={({ match }) => (
              <AccountDetails address={match.params.address} />
            )}
          />
          <Route
            exact
            path={ACCOUNT_ALIASES_PLURAL.map((alias) => "/" + alias)}
          >
            <TabbedPage tab="Accounts">
              <AccountsCard />
            </TabbedPage>
          </Route>
          <Route>
            <TabbedPage tab="Stats">
              <StatsCard />
              <SolanabeachComponents />
            </TabbedPage>
          </Route>
        </Switch>
      </div>
    </>
  );
}

export default App;
