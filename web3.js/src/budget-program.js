// @flow

import * as BufferLayout from 'buffer-layout';

import {Transaction} from './transaction';
import {PublicKey} from './publickey';
import * as Layout from './layout';

/**
 * Represents a condition that is met by executing a `applySignature()`
 * transaction
 *
 * @typedef {Object} SignatureCondition
 * @property {string} type Must equal the string 'timestamp'
 * @property {PublicKey} from Public key from which `applySignature()` will be accepted from
 */
export type SignatureCondition = {
  type: 'signature',
  from: PublicKey,
};

/**
 * Represents a condition that is met by executing a `applyTimestamp()`
 * transaction
 *
 * @typedef {Object} TimestampCondition
 * @property {string} type Must equal the string 'timestamp'
 * @property {PublicKey} from Public key from which `applyTimestamp()` will be accepted from
 * @property {Date} when The timestamp that was observed
 */
export type TimestampCondition = {
  type: 'timestamp',
  from: PublicKey,
  when: Date,
};

/**
 * Represents a payment to a given public key
 *
 * @typedef {Object} Payment
 * @property {number} amount Number of tokens
 * @property {PublicKey} to Public key of the recipient
 */
export type Payment = {
  amount: number,
  to: PublicKey,
};

/**
 * A condition that can unlock a payment
 *
 * @typedef {SignatureCondition|TimestampCondition} BudgetCondition
 */
export type BudgetCondition = SignatureCondition | TimestampCondition;

/**
 * @private
 */
function serializePayment(payment: Payment): Buffer {
  const toData = payment.to.toBuffer();
  const userdata = Buffer.alloc(8 + toData.length);
  userdata.writeUInt32LE(payment.amount, 0);
  toData.copy(userdata, 8);
  return userdata;
}

/**
 * @private
 */
function serializeDate(when: Date): Buffer {
  const userdata = Buffer.alloc(8 + 20);
  userdata.writeUInt32LE(20, 0); // size of timestamp as u64

  function iso(date) {
    function pad(number) {
      if (number < 10) {
        return '0' + number;
      }
      return number;
    }

    return (
      date.getUTCFullYear() +
      '-' +
      pad(date.getUTCMonth() + 1) +
      '-' +
      pad(date.getUTCDate()) +
      'T' +
      pad(date.getUTCHours()) +
      ':' +
      pad(date.getUTCMinutes()) +
      ':' +
      pad(date.getUTCSeconds()) +
      'Z'
    );
  }
  userdata.write(iso(when), 8);
  return userdata;
}

/**
 * @private
 */
function serializeCondition(condition: BudgetCondition) {
  switch (condition.type) {
    case 'timestamp': {
      const date = serializeDate(condition.when);
      const from = condition.from.toBuffer();

      const userdata = Buffer.alloc(4 + date.length + from.length);
      userdata.writeUInt32LE(0, 0); // Condition enum = Timestamp
      date.copy(userdata, 4);
      from.copy(userdata, 4 + date.length);
      return userdata;
    }
    case 'signature': {
      const userdataLayout = BufferLayout.struct([
        BufferLayout.u32('condition'),
        Layout.publicKey('from'),
      ]);

      const from = condition.from.toBuffer();
      const userdata = Buffer.alloc(4 + from.length);
      userdataLayout.encode(
        {
          instruction: 1, // Signature
          from,
        },
        userdata,
      );
      return userdata;
    }
    default:
      throw new Error(`Unknown condition type: ${condition.type}`);
  }
}

/**
 * Factory class for transactions to interact with the Budget program
 */
export class BudgetProgram {
  /**
   * Public key that identifies the Budget program
   */
  static get programId(): PublicKey {
    return new PublicKey(
      '0x8100000000000000000000000000000000000000000000000000000000000000',
    );
  }

  /**
   * The amount of space this program requires
   */
  static get space(): number {
    return 128;
  }

  /**
   * Creates a timestamp condition
   */
  static timestampCondition(from: PublicKey, when: Date): TimestampCondition {
    return {
      type: 'timestamp',
      from,
      when,
    };
  }

  /**
   * Creates a signature condition
   */
  static signatureCondition(from: PublicKey): SignatureCondition {
    return {
      type: 'signature',
      from,
    };
  }

  /**
   * Generates a transaction that transfers tokens once any of the conditions are met
   */
  static pay(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
    amount: number,
    ...conditions: Array<BudgetCondition>
  ): Transaction {
    const userdata = Buffer.alloc(1024);
    let pos = 0;
    userdata.writeUInt32LE(0, pos); // NewBudget instruction
    pos += 4;

    switch (conditions.length) {
      case 0:
        userdata.writeUInt32LE(0, pos); // Budget enum = Pay
        pos += 4;

        {
          const payment = serializePayment({amount, to});
          payment.copy(userdata, pos);
          pos += payment.length;
        }

        return new Transaction().add({
          keys: [from, to],
          programId: this.programId,
          userdata: userdata.slice(0, pos),
        });
      case 1:
        userdata.writeUInt32LE(1, pos); // Budget enum = After
        pos += 4;
        {
          const condition = conditions[0];

          const conditionData = serializeCondition(condition);
          conditionData.copy(userdata, pos);
          pos += conditionData.length;

          const paymentData = serializePayment({amount, to});
          paymentData.copy(userdata, pos);
          pos += paymentData.length;
        }

        return new Transaction().add({
          keys: [from, program, to],
          programId: this.programId,
          userdata: userdata.slice(0, pos),
        });

      case 2:
        userdata.writeUInt32LE(2, pos); // Budget enum = Or
        pos += 4;

        for (let condition of conditions) {
          const conditionData = serializeCondition(condition);
          conditionData.copy(userdata, pos);
          pos += conditionData.length;

          const paymentData = serializePayment({amount, to});
          paymentData.copy(userdata, pos);
          pos += paymentData.length;
        }

        return new Transaction().add({
          keys: [from, program, to],
          programId: this.programId,
          userdata: userdata.slice(0, pos),
        });

      default:
        throw new Error(
          `A maximum of two conditions are support: ${
            conditions.length
          } provided`,
        );
    }
  }

  /**
   * Generates a transaction that transfers tokens once both conditions are met
   */
  static payOnBoth(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
    amount: number,
    condition1: BudgetCondition,
    condition2: BudgetCondition,
  ): Transaction {
    const userdata = Buffer.alloc(1024);
    let pos = 0;
    userdata.writeUInt32LE(0, pos); // NewBudget instruction
    pos += 4;

    userdata.writeUInt32LE(3, pos); // Budget enum = And
    pos += 4;

    for (let condition of [condition1, condition2]) {
      const conditionData = serializeCondition(condition);
      conditionData.copy(userdata, pos);
      pos += conditionData.length;
    }

    const paymentData = serializePayment({amount, to});
    paymentData.copy(userdata, pos);
    pos += paymentData.length;

    return new Transaction().add({
      keys: [from, program, to],
      programId: this.programId,
      userdata: userdata.slice(0, pos),
    });
  }

  /**
   * Generates a transaction that applies a timestamp, which could enable a
   * pending payment to proceed.
   */
  static applyTimestamp(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
    when: Date,
  ): Transaction {
    const whenData = serializeDate(when);
    const userdata = Buffer.alloc(4 + whenData.length);

    userdata.writeUInt32LE(1, 0); // ApplyTimestamp instruction
    whenData.copy(userdata, 4);

    return new Transaction().add({
      keys: [from, program, to],
      programId: this.programId,
      userdata,
    });
  }

  /**
   * Generates a transaction that applies a signature, which could enable a
   * pending payment to proceed.
   */
  static applySignature(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
  ): Transaction {
    const userdataLayout = BufferLayout.struct([
      BufferLayout.u32('instruction'),
    ]);

    const userdata = Buffer.alloc(userdataLayout.span);
    userdataLayout.encode(
      {
        instruction: 2, // ApplySignature instruction
      },
      userdata,
    );

    return new Transaction().add({
      keys: [from, program, to],
      programId: this.programId,
      userdata,
    });
  }
}
