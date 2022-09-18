import React, { Suspense } from "react";
import { Account } from "../../../providers/accounts";
import { CachedImageContent } from "../../common/NFTArt";
import { isGlowIdAccount } from "./isGlowIdAccount";
import { useParseGlowID } from "./useParseGlowID";

export function GlowIdAccountHeader({ account }: { account: Account }) {
  const glowId = isGlowIdAccount(account);

  if (glowId) {
    return (
      <Suspense fallback={<div>Loading...</div>}>
        <GlowIdWithDataHeader account={account} />
      </Suspense>
    );
  }

  return (
    <>
      <h6 className="header-pretitle">Details</h6>
      <h2 className="header-title">Account</h2>
    </>
  );
}

function GlowIdWithDataHeader({ account }: { account: Account }) {
  const { data: glowId } = useParseGlowID(account);

  if (!glowId) {
    return (
      <>
        <h6 className="header-pretitle">Details</h6>
        <h2 className="header-title">Account</h2>
      </>
    );
  }

  return (
    <div className="row">
      <div className="col-auto ms-2 d-flex align-items-center">
        <CachedImageContent uri={glowId?.image ?? undefined} />
      </div>

      <div className="col mb-3 ms-0.5 mt-3">
        <h6 className="header-pretitle ms-1">Glow ID</h6>

        <h2 className="header-title align-items-center no-overflow-with-ellipsis">
          {glowId?.name || "No Name"}
        </h2>

        <h2 className="header-subtitle align-items-center">
          {glowId.handle}.glow
        </h2>
      </div>
    </div>
  );
}

