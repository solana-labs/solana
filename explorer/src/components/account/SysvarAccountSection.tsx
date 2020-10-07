import React from "react";

import { Account, useFetchAccountInfo } from "providers/accounts";
import { SysvarAccount } from "validators/accounts/sysvar";
import { TableCardBody } from "components/common/TableCardBody";
import { AccountHeader, AccountAddressRow } from "components/common/Account";

export function SysvarAccountSection({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  switch (sysvarAccount.type) {
    case "clock":
      return (
        <SysvarAccountClockCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "rent":
      return (
        <SysvarAccountRentCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "rewards":
      return (
        <SysvarAccountRewardsCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "epochSchedule":
      return (
        <SysvarAccountEpochScheduleCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "fees":
      return (
        <SysvarAccountFeesCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "recentBlockhashes":
      return (
        <SysvarAccountRecentBlockhashesCard
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "slotHashes":
      return (
        <SysvarAccountSlotHashes
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "slotHistory":
      return (
        <SysvarAccountSlotHistory
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
    case "stakeHistory":
      return (
        <SysvarAccountStakeHistory
          account={account}
          sysvarAccount={sysvarAccount}
        />
      );
  }
}

function SysvarAccountRecentBlockhashesCard({
  account,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Recent Blockhashes"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
      </TableCardBody>
    </div>
  );
}

function SysvarAccountSlotHashes({
  account,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Slot Hashes"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
      </TableCardBody>
    </div>
  );
}

function SysvarAccountSlotHistory({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Slot History"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Next Slot</td>
          <td className="text-lg-right">{sysvarAccount.info.nextSlot}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function SysvarAccountStakeHistory({
  account,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Stake History"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />
      </TableCardBody>
    </div>
  );
}

function SysvarAccountFeesCard({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader title="Fees" refresh={() => refresh(account.pubkey)} />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Lamports Per Signature</td>
          <td className="text-lg-right">
            {sysvarAccount.info.feeCalculator.lamportsPerSignature}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function SysvarAccountEpochScheduleCard({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Epoch Schedule"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Slots Per Epoch</td>
          <td className="text-lg-right">{sysvarAccount.info.slotsPerEpoch}</td>
        </tr>

        <tr>
          <td>Leader Schedule Slot Offset</td>
          <td className="text-lg-right">
            {sysvarAccount.info.leaderScheduleSlotOffset}
          </td>
        </tr>

        <tr>
          <td>Epoch Warmup Enabled</td>
          <td className="text-lg-right">
            <code>{sysvarAccount.info.warmup ? "true" : "false"}</code>
          </td>
        </tr>

        <tr>
          <td>First Normal Epoch</td>
          <td className="text-lg-right">
            {sysvarAccount.info.firstNormalEpoch}
          </td>
        </tr>

        <tr>
          <td>First Normal Slot</td>
          <td className="text-lg-right">
            {sysvarAccount.info.firstNormalSlot}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function SysvarAccountClockCard({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader
        title="Clock Sysvar"
        refresh={() => refresh(account.pubkey)}
      />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Unix Timestamp</td>
          <td className="text-lg-right">{sysvarAccount.info.unixTimestamp}</td>
        </tr>

        <tr>
          <td>Epoch</td>
          <td className="text-lg-right">{sysvarAccount.info.epoch}</td>
        </tr>

        <tr>
          <td>Leader Schedule Epoch</td>
          <td className="text-lg-right">
            {sysvarAccount.info.leaderScheduleEpoch}
          </td>
        </tr>

        <tr>
          <td>Slot</td>
          <td className="text-lg-right">{sysvarAccount.info.slot}</td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function SysvarAccountRentCard({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader title="Rent" refresh={() => refresh(account.pubkey)} />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Burn Percent</td>
          <td className="text-lg-right">
            {sysvarAccount.info.burnPercent + "%"}
          </td>
        </tr>

        <tr>
          <td>Exemption Threshold</td>
          <td className="text-lg-right">
            {sysvarAccount.info.exemptionThreshold}
          </td>
        </tr>

        <tr>
          <td>Lamports Per Byte Year</td>
          <td className="text-lg-right">
            {sysvarAccount.info.lamportsPerByteYear}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}

function SysvarAccountRewardsCard({
  account,
  sysvarAccount,
}: {
  account: Account;
  sysvarAccount: SysvarAccount;
}) {
  const refresh = useFetchAccountInfo();
  return (
    <div className="card">
      <AccountHeader title="Rewards" refresh={() => refresh(account.pubkey)} />

      <TableCardBody>
        <AccountAddressRow account={account} />

        <tr>
          <td>Validator Point Value</td>
          <td className="text-lg-right text-monospace">
            {sysvarAccount.info.validatorPointValue}
          </td>
        </tr>
      </TableCardBody>
    </div>
  );
}
