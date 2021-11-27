import fs from 'mz/fs';
import {expect, use} from 'chai';
import chaiAsPromised from 'chai-as-promised';

import {
  Connection,
  BpfLoaderUpgradeable,
  Keypair,
  BpfLoaderUpgradeableProgram,
} from '../src';
import {url} from './url';
import {helpers} from './mocks/rpc-http';

use(chaiAsPromised);

if (process.env.TEST_LIVE) {
  describe('BPF Loader Upgradeable', () => {
    const connection = new Connection(url, 'confirmed');
    let programData: Buffer;

    before(async function () {
      programData = await fs.readFile(
        'test/fixtures/noop-program/solana_bpf_rust_noop.so',
      );
    });

    // create + load + authority + close
    it('Buffer lifecycle', async function () {
      this.timeout(60_000);

      const payerAccount = Keypair.generate();
      const bufferAccount = Keypair.generate();
      const authorityAccount = Keypair.generate();
      const authorityAccount2 = Keypair.generate();

      const {feeCalculator} = await connection.getRecentBlockhash();
      const fees =
        feeCalculator.lamportsPerSignature *
        // createAccount
        (2 +
          // loadBuffer
          Math.ceil(
            programData.length / BpfLoaderUpgradeable.WRITE_CHUNK_SIZE,
          ) *
            2 +
          // setBufferAuthority
          2 +
          // closeBuffer
          2 * 2);
      const payerBalance = await connection.getMinimumBalanceForRentExemption(
        0,
      );
      const bufferAccountSize = BpfLoaderUpgradeable.getBufferAccountSize(
        programData.length,
      );
      const bufferAccountBalance =
        await connection.getMinimumBalanceForRentExemption(bufferAccountSize);

      await helpers.airdrop({
        connection,
        address: payerAccount.publicKey,
        amount: payerBalance + bufferAccountBalance + fees,
      });

      await BpfLoaderUpgradeable.createBuffer(
        connection,
        payerAccount,
        bufferAccount,
        authorityAccount.publicKey,
        bufferAccountBalance,
        programData.length,
      );

      await BpfLoaderUpgradeable.loadBuffer(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        authorityAccount,
        programData,
      );

      await BpfLoaderUpgradeable.setBufferAuthority(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        authorityAccount,
        authorityAccount2.publicKey,
      );

      await expect(
        BpfLoaderUpgradeable.closeBuffer(
          connection,
          payerAccount,
          bufferAccount.publicKey,
          authorityAccount,
          payerAccount.publicKey,
        ),
      ).to.be.rejected;

      await BpfLoaderUpgradeable.closeBuffer(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        authorityAccount2,
        payerAccount.publicKey,
      );

      expect(await connection.getAccountInfo(bufferAccount.publicKey)).to.be
        .null;
    });

    // create buffer + write buffer + deploy + upgrade + set authority + close
    it('Program lifecycle', async function () {
      this.timeout(60_000);

      const payerAccount = Keypair.generate();
      const bufferAccount = Keypair.generate();
      const bufferAuthorityAccount = Keypair.generate();
      const programAccount = Keypair.generate();
      const programAuthorityAccount = Keypair.generate();

      const {feeCalculator} = await connection.getRecentBlockhash();
      const fees =
        feeCalculator.lamportsPerSignature *
        // createAccount
        (2 +
          // loadBuffer
          Math.ceil(
            programData.length / BpfLoaderUpgradeable.WRITE_CHUNK_SIZE,
          ) *
            2 +
          // deployProgram
          3 +
          // setProgramAuthority
          2 +
          // closeProgram
          2 * 2);
      const payerBalance = await connection.getMinimumBalanceForRentExemption(
        0,
      );
      const bufferAccountSize = BpfLoaderUpgradeable.getBufferAccountSize(
        programData.length,
      );
      const bufferAccountBalance =
        await connection.getMinimumBalanceForRentExemption(bufferAccountSize);
      const programAccountSize = BpfLoaderUpgradeable.getBufferAccountSize(
        BpfLoaderUpgradeable.BUFFER_PROGRAM_SIZE,
      );
      const programAccountBalance =
        await connection.getMinimumBalanceForRentExemption(programAccountSize);
      const programDataAccountSize =
        BpfLoaderUpgradeable.BUFFER_PROGRAM_DATA_HEADER_SIZE +
        programData.length * 2;
      const programDataAccountBalance =
        await connection.getMinimumBalanceForRentExemption(
          programDataAccountSize,
        );

      await helpers.airdrop({
        connection,
        address: payerAccount.publicKey,
        amount:
          payerBalance +
          bufferAccountBalance * 2 +
          programAccountSize +
          programDataAccountBalance +
          fees,
      });

      await BpfLoaderUpgradeable.createBuffer(
        connection,
        payerAccount,
        bufferAccount,
        bufferAuthorityAccount.publicKey,
        bufferAccountBalance,
        programData.length,
      );

      await BpfLoaderUpgradeable.loadBuffer(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        bufferAuthorityAccount,
        programData,
      );

      await BpfLoaderUpgradeable.deployProgram(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        bufferAuthorityAccount,
        programAccount,
        programAccountBalance,
        programData.length * 2,
      );

      expect(await connection.getAccountInfo(bufferAccount.publicKey)).to.be
        .null;

      // Upgrade
      await BpfLoaderUpgradeable.createBuffer(
        connection,
        payerAccount,
        bufferAccount,
        bufferAuthorityAccount.publicKey,
        bufferAccountBalance,
        programData.length,
      );

      await BpfLoaderUpgradeable.loadBuffer(
        connection,
        payerAccount,
        bufferAccount.publicKey,
        bufferAuthorityAccount,
        programData,
      );

      await BpfLoaderUpgradeable.upgradeProgram(
        connection,
        payerAccount,
        programAccount.publicKey,
        bufferAuthorityAccount,
        bufferAccount.publicKey,
        payerAccount.publicKey,
      );

      expect(await connection.getAccountInfo(bufferAccount.publicKey)).to.be
        .null;

      // failed close + set authority + close
      await expect(
        BpfLoaderUpgradeable.closeProgram(
          connection,
          payerAccount,
          programAccount.publicKey,
          programAuthorityAccount,
          payerAccount.publicKey,
        ),
      ).to.be.rejected;

      await BpfLoaderUpgradeable.setProgramAuthority(
        connection,
        payerAccount,
        programAccount.publicKey,
        bufferAuthorityAccount,
        programAuthorityAccount.publicKey,
      );

      await BpfLoaderUpgradeable.closeProgram(
        connection,
        payerAccount,
        programAccount.publicKey,
        programAuthorityAccount,
        payerAccount.publicKey,
      );

      const programDataAccount =
        await BpfLoaderUpgradeableProgram.getProgramDataAddress(
          programAccount.publicKey,
        );
      expect(await connection.getAccountInfo(programDataAccount)).to.be.null;
    });
  });
}
