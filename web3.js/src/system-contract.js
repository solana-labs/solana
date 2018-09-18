// @flow

import assert from 'assert';

import {Transaction} from './transaction';
import type {PublicKey} from './account';

/**
 * Factory class for transactions to interact with the System contract
 */
export class SystemContract {
  /**
   * Public key that identifies the System Contract
   */
  static get contractId(): PublicKey {
    return '11111111111111111111111111111111';
  }

  /**
   * Generate a Transaction that creates a new account
   */
  static createAccount(
    from: PublicKey,
    newAccount: PublicKey,
    tokens: number,
    space: number,
    contractId: ?PublicKey
  ): Transaction {
    const userdata = Buffer.alloc(4 + 8 + 8 + 1 + 32);
    let pos = 0;

    userdata.writeUInt32LE(0, pos); // Create Account instruction
    pos += 4;

    userdata.writeUInt32LE(tokens, pos); // tokens as i64
    pos += 8;

    userdata.writeUInt32LE(space, pos); // space as u64
    pos += 8;

    if (contractId) {
      userdata.writeUInt8(1, pos); // 'Some'
      pos += 1;

      const contractIdBytes = Transaction.serializePublicKey(contractId);
      contractIdBytes.copy(userdata, pos);
      pos += 32;
    } else {
      userdata.writeUInt8(0, pos); // 'None'
      pos += 1;
    }

    assert(pos <= userdata.length);

    return new Transaction({
      fee: 0,
      keys: [from, newAccount],
      contractId: SystemContract.contractId,
      userdata,
    });
  }

  /**
   * Generate a Transaction that moves tokens from one account to another
   */
  static move(from: PublicKey, to: PublicKey, amount: number): Transaction {
    const userdata = Buffer.alloc(4 + 8);
    let pos = 0;
    userdata.writeUInt32LE(2, pos); // Move instruction
    pos += 4;

    userdata.writeUInt32LE(amount, pos); // amount as u64
    pos += 8;

    assert(pos === userdata.length);

    return new Transaction({
      fee: 0,
      keys: [from, to],
      contractId: SystemContract.contractId,
      userdata,
    });
  }

  /**
   * Generate a Transaction that assigns an account to a contract id
   */
  static assign(from: PublicKey, contractId: PublicKey): Transaction {
    const userdata = Buffer.alloc(4 + 32);
    let pos = 0;

    userdata.writeUInt32LE(1, pos); // Assign instruction
    pos += 4;

    const contractIdBytes = Transaction.serializePublicKey(contractId);
    contractIdBytes.copy(userdata, pos);
    pos += contractIdBytes.length;

    assert(pos === userdata.length);

    return new Transaction({
      fee: 0,
      keys: [from],
      contractId: SystemContract.contractId,
      userdata,
    });
  }
}
