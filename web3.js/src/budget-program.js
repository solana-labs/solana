// @flow

import {Transaction} from './transaction';
import type {PublicKey} from './account';

/**
 * Represents a condition that is met by executing a `applySignature()`
 * transaction
 *
 * @typedef {Object} SignatureCondition
 * @property {string} type Must equal the string 'timestamp'
 * @property {PublicKey} from Public key from which `applySignature()` will be accepted from
 */
export type SignatureCondition = {
  type: 'signature';
  from: PublicKey;
};

/**
 * Represents a condition that is met by executing a `applyTimestamp()`
 * transaction
 *
 * @typedef {Object} TimeStampCondition
 * @property {string} type Must equal the string 'timestamp'
 * @property {PublicKey} from Public key from which `applyTimestamp()` will be accepted from
 * @property {Date} when The timestamp that was observed
 */
export type TimeStampCondition = {
  type: 'timestamp';
  from: PublicKey;
  when: Date;
};

/**
 * Represents a payment to a given public key
 *
 * @typedef {Object} Payment
 * @property {number} amount Number of tokens
 * @property {PublicKey} to Public key of the recipient
 */
export type Payment = {
  amount: number;
  to: PublicKey;
}

/**
 * A condition that can unlock a payment
 *
 * @typedef {SignatureCondition|TimeStampCondition} BudgetCondition
 */
export type BudgetCondition = SignatureCondition | TimeStampCondition;

/**
 * @private
 */
function serializePayment(payment: Payment): Buffer {
  const toData = Transaction.serializePublicKey(payment.to);
  const userdata = Buffer.alloc(8 + toData.length);
  userdata.writeUInt32LE(payment.amount, 0);
  toData.copy(userdata, 8);
  return userdata;
}

/**
 * @private
 */
function serializeDate(
  when: Date
): Buffer {
  const userdata = Buffer.alloc(8 + 20);
  userdata.writeUInt32LE(20, 0); // size of timestamp as u64

  function iso(date) {
    function pad(number) {
      if (number < 10) {
        return '0' + number;
      }
      return number;
    }

    return date.getUTCFullYear() +
      '-' + pad(date.getUTCMonth() + 1) +
      '-' + pad(date.getUTCDate()) +
      'T' + pad(date.getUTCHours()) +
      ':' + pad(date.getUTCMinutes()) +
      ':' + pad(date.getUTCSeconds()) +
      'Z';
  }
  userdata.write(iso(when), 8);
  return userdata;
}

/**
 * @private
 */
function serializeCondition(condition: BudgetCondition) {
  switch(condition.type) {
  case 'timestamp':
  {
    const date = serializeDate(condition.when);
    const from = Transaction.serializePublicKey(condition.from);

    const userdata = Buffer.alloc(4 + date.length + from.length);
    userdata.writeUInt32LE(0, 0); // Condition enum = Timestamp
    date.copy(userdata, 4);
    from.copy(userdata, 4 + date.length);
    return userdata;
  }
  case 'signature':
  {
    const from = Transaction.serializePublicKey(condition.from);

    const userdata = Buffer.alloc(4 + from.length);
    userdata.writeUInt32LE(1, 0); // Condition enum = Signature
    from.copy(userdata, 4);
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
    return '4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM';
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
  static timestampCondition(from: PublicKey, when: Date) : TimeStampCondition {
    return {
      type: 'timestamp',
      from,
      when,
    };
  }

  /**
   * Creates a signature condition
   */
  static signatureCondition(from: PublicKey) : SignatureCondition {
    return {
      type: 'signature',
      from,
    };
  }

  /**
   * Generates a transaction that transfer tokens once a set of conditions are
   * met
   */
  static pay(
    from: PublicKey,
    contract: PublicKey,
    to: PublicKey,
    amount: number,
    ...conditions: Array<BudgetCondition>
  ): Transaction {

    const userdata = Buffer.alloc(1024);
    let pos = 0;
    userdata.writeUInt32LE(0, pos); // NewContract instruction
    pos += 4;

    userdata.writeUInt32LE(amount, pos); // Contract.tokens
    pos += 8;

    switch (conditions.length) {
    case 0:
      userdata.writeUInt32LE(0, pos); // Budget enum = Pay
      pos += 4;

      {
        const payment = serializePayment({amount, to});
        payment.copy(userdata, pos);
        pos += payment.length;
      }

      return new Transaction({
        fee: 0,
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

      return new Transaction({
        fee: 0,
        keys: [from, contract, to],
        programId: this.programId,
        userdata: userdata.slice(0, pos),
      });

    case 2:
      userdata.writeUInt32LE(2, pos); // Budget enum = Ok
      pos += 4;

      for (let condition of conditions) {
        const conditionData = serializeCondition(condition);
        conditionData.copy(userdata, pos);
        pos += conditionData.length;

        const paymentData = serializePayment({amount, to});
        paymentData.copy(userdata, pos);
        pos += paymentData.length;
      }

      return new Transaction({
        fee: 0,
        keys: [from, contract, to],
        programId: this.programId,
        userdata: userdata.slice(0, pos),
      });

    default:
      throw new Error(`A maximum of two conditions are support: ${conditions.length} provided`);
    }
  }

  static applyTimestamp(from: PublicKey, contract: PublicKey, to: PublicKey, when: Date): Transaction {
    const whenData = serializeDate(when);
    const userdata = Buffer.alloc(4 + whenData.length);

    userdata.writeUInt32LE(1, 0); // ApplyTimestamp instruction
    whenData.copy(userdata, 4);

    return new Transaction({
      fee: 0,
      keys: [from, contract, to],
      programId: this.programId,
      userdata,
    });
  }

  static applySignature(from: PublicKey, contract: PublicKey, to: PublicKey): Transaction {
    const userdata = Buffer.alloc(4);
    userdata.writeUInt32LE(2, 0); // ApplySignature instruction

    return new Transaction({
      fee: 0,
      keys: [from, contract, to],
      programId: this.programId,
      userdata,
    });
  }
}
