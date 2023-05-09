import React from "react";
import Layout from "@theme/Layout";
import DocSidebar from "@theme/DocSidebar";
import SidebarStyles from "@docusaurus/theme-classic/lib/theme/DocPage/Layout/Sidebar/styles.module.css";
import DocPageStyles from "@docusaurus/theme-classic/lib/theme/DocPage/Layout/styles.module.css";
import sidebar from "../sidebars";

function CardLayout({
  children,
  sidebarKey = false,
  title = "",
  description = "",
  path = "",
}) {
  // load the sidebar item from the master `sidebars.js` file
  let sidebarItems = (sidebarKey && sidebar?.[sidebarKey]) || [];

  // process each of the loaded sidebar items for formatting
  if (sidebarItems?.length) sidebarItems = parseSidebar(sidebarItems);

  // return the page layout, ready to go
  return (
    <Layout title={title} description={description}>
      <div className={DocPageStyles.docPage}>
        {sidebarItems?.length > 0 && (
          <aside className={SidebarStyles.docSidebarContainer}>
            <DocSidebar sidebar={sidebarItems} path={path}></DocSidebar>
          </aside>
        )}

        <main className={DocPageStyles.docPage}>{children}</main>
      </div>
    </Layout>
  );
}
export default CardLayout;

/*
  Create a simple label based on the string of a doc file path
*/
const computeLabel = (label) => {
  label = label.split("/");
  label = label[label?.length - 1]?.replace("-", " ");
  label = label.charAt(0).toUpperCase() + label.slice(1);
  return label && label;
};

/*
  Recursively parse the sidebar
*/
const parseSidebar = (sidebarItems) => {
  Object.keys(sidebarItems).forEach((key) => {
    if (sidebarItems[key]?.type?.toLowerCase() === "category") {
      sidebarItems[key].items = parseSidebar(sidebarItems[key].items);
    } else sidebarItems[key] = formatter(sidebarItems[key]);
  });
  return sidebarItems;
};

/*
  Parser to format a sidebar item to be compatible with the `DocSidebar` component
*/
const formatter = (item) => {
  // handle string only document ids
  if (typeof item === "string") {
    item = {
      type: "link",
      href: item,
      label: computeLabel(item) || item || "[unknown label]",
    };
  }

  // handle object style docs
  else if (item?.type?.toLowerCase() === "doc") {
    item.type = "link";
    item.href = item.id;
    item.label = item.label || computeLabel(item.href) || "[unknown label]";
    delete item.id;
  }

  // fix for local routing that does not specify starting at the site root
  if (
    !(
      item?.href?.startsWith("/") ||
      item?.href?.startsWith("http:") ||
      item?.href?.startsWith("https")
    )
  )
    item.href = `/${item?.href}`;

  return item;
};
