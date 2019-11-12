// @flow

import * as BufferLayout from 'buffer-layout';

import {Transaction} from './transaction';
import {PublicKey} from './publickey';
import {SystemProgram} from './system-program';

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
 * @property {number} amount Number of lamports
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
  const data = Buffer.alloc(8 + toData.length);
  data.writeUInt32LE(payment.amount, 0);
  toData.copy(data, 8);
  return data;
}

/**
 * @private
 */
function serializeDate(when: Date): Buffer {
  const data = Buffer.alloc(8 + 20);
  data.writeUInt32LE(20, 0); // size of timestamp as u64

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
  data.write(iso(when), 8);
  return data;
}

/**
 * @private
 */
function serializeCondition(condition: BudgetCondition) {
  switch (condition.type) {
    case 'timestamp': {
      const date = serializeDate(condition.when);
      const from = condition.from.toBuffer();

      const data = Buffer.alloc(4 + date.length + from.length);
      data.writeUInt32LE(0, 0); // Condition enum = Timestamp
      date.copy(data, 4);
      from.copy(data, 4 + date.length);
      return data;
    }
    case 'signature': {
      const from = condition.from.toBuffer();
      const data = Buffer.alloc(4 + from.length);
      data.writeUInt32LE(1, 0); // Condition enum = Signature
      from.copy(data, 4);
      return data;
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
    return new PublicKey('Budget1111111111111111111111111111111111111');
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
   * Generates a transaction that transfers lamports once any of the conditions are met
   */
  static pay(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
    amount: number,
    ...conditions: Array<BudgetCondition>
  ): Transaction {
    const data = Buffer.alloc(1024);
    let pos = 0;
    data.writeUInt32LE(0, pos); // NewBudget instruction
    pos += 4;

    switch (conditions.length) {
      case 0: {
        data.writeUInt32LE(0, pos); // BudgetExpr enum = Pay
        pos += 4;

        {
          const payment = serializePayment({amount, to});
          payment.copy(data, pos);
          pos += payment.length;
        }
        const trimmedData = data.slice(0, pos);

        const transaction = SystemProgram.createAccount(
          from,
          program,
          amount,
          trimmedData.length,
          this.programId,
        );

        return transaction.add({
          keys: [
            {pubkey: to, isSigner: false, isWritable: true},
            {pubkey: program, isSigner: false, isWritable: true},
          ],
          programId: this.programId,
          data: trimmedData,
        });
      }
      case 1: {
        data.writeUInt32LE(1, pos); // BudgetExpr enum = After
        pos += 4;
        {
          const condition = conditions[0];

          const conditionData = serializeCondition(condition);
          conditionData.copy(data, pos);
          pos += conditionData.length;

          data.writeUInt32LE(0, pos); // BudgetExpr enum = Pay
          pos += 4;

          const paymentData = serializePayment({amount, to});
          paymentData.copy(data, pos);
          pos += paymentData.length;
        }
        const trimmedData = data.slice(0, pos);

        const transaction = SystemProgram.createAccount(
          from,
          program,
          amount,
          trimmedData.length,
          this.programId,
        );

        return transaction.add({
          keys: [{pubkey: program, isSigner: false, isWritable: true}],
          programId: this.programId,
          data: trimmedData,
        });
      }

      case 2: {
        data.writeUInt32LE(2, pos); // BudgetExpr enum = Or
        pos += 4;

        for (let condition of conditions) {
          const conditionData = serializeCondition(condition);
          conditionData.copy(data, pos);
          pos += conditionData.length;

          data.writeUInt32LE(0, pos); // BudgetExpr enum = Pay
          pos += 4;

          const paymentData = serializePayment({amount, to});
          paymentData.copy(data, pos);
          pos += paymentData.length;
        }
        const trimmedData = data.slice(0, pos);

        const transaction = SystemProgram.createAccount(
          from,
          program,
          amount,
          trimmedData.length,
          this.programId,
        );

        return transaction.add({
          keys: [{pubkey: program, isSigner: false, isWritable: true}],
          programId: this.programId,
          data: trimmedData,
        });
      }

      default:
        throw new Error(
          `A maximum of two conditions are supported: ${conditions.length} provided`,
        );
    }
  }

  /**
   * Generates a transaction that transfers lamports once both conditions are met
   */
  static payOnBoth(
    from: PublicKey,
    program: PublicKey,
    to: PublicKey,
    amount: number,
    condition1: BudgetCondition,
    condition2: BudgetCondition,
  ): Transaction {
    const data = Buffer.alloc(1024);
    let pos = 0;
    data.writeUInt32LE(0, pos); // NewBudget instruction
    pos += 4;

    data.writeUInt32LE(3, pos); // BudgetExpr enum = And
    pos += 4;

    for (let condition of [condition1, condition2]) {
      const conditionData = serializeCondition(condition);
      conditionData.copy(data, pos);
      pos += conditionData.length;
    }

    data.writeUInt32LE(0, pos); // BudgetExpr enum = Pay
    pos += 4;

    const paymentData = serializePayment({amount, to});
    paymentData.copy(data, pos);
    pos += paymentData.length;

    const trimmedData = data.slice(0, pos);

    const transaction = SystemProgram.createAccount(
      from,
      program,
      amount,
      trimmedData.length,
      this.programId,
    );

    return transaction.add({
      keys: [{pubkey: program, isSigner: false, isWritable: true}],
      programId: this.programId,
      data: trimmedData,
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
    const data = Buffer.alloc(4 + whenData.length);

    data.writeUInt32LE(1, 0); // ApplyTimestamp instruction
    whenData.copy(data, 4);

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true, isWritable: true},
        {pubkey: program, isSigner: false, isWritable: true},
        {pubkey: to, isSigner: false, isWritable: true},
      ],
      programId: this.programId,
      data,
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
    const dataLayout = BufferLayout.struct([BufferLayout.u32('instruction')]);

    const data = Buffer.alloc(dataLayout.span);
    dataLayout.encode(
      {
        instruction: 2, // ApplySignature instruction
      },
      data,
    );

    return new Transaction().add({
      keys: [
        {pubkey: from, isSigner: true, isWritable: true},
        {pubkey: program, isSigner: false, isWritable: true},
        {pubkey: to, isSigner: false, isWritable: true},
      ],
      programId: this.programId,
      data,
    });
  }
}
