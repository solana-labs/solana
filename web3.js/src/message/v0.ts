import bs58 from 'bs58';
import * as BufferLayout from '@solana/buffer-layout';

import * as Layout from '../layout';
import {Blockhash} from '../blockhash';
import {
  MessageHeader,
  MessageAddressTableLookup,
  MessageCompiledInstruction,
} from './index';
import {PublicKey, PUBLIC_KEY_LENGTH} from '../publickey';
import * as shortvec from '../utils/shortvec-encoding';
import assert from '../utils/assert';
import {PACKET_DATA_SIZE, VERSION_PREFIX_MASK} from '../transaction/constants';
import {TransactionInstruction} from '../transaction';
import {AddressLookupTableAccount} from '../programs';
import {CompiledKeys} from './compiled-keys';
import {AccountKeysFromLookups, MessageAccountKeys} from './account-keys';

/**
 * Message constructor arguments
 */
export type MessageV0Args = {
  /** The message header, identifying signed and read-only `accountKeys` */
  header: MessageHeader;
  /** The static account keys used by this transaction */
  staticAccountKeys: PublicKey[];
  /** The hash of a recent ledger block */
  recentBlockhash: Blockhash;
  /** Instructions that will be executed in sequence and committed in one atomic transaction if all succeed. */
  compiledInstructions: MessageCompiledInstruction[];
  /** Instructions that will be executed in sequence and committed in one atomic transaction if all succeed. */
  addressTableLookups: MessageAddressTableLookup[];
};

export type CompileV0Args = {
  payerKey: PublicKey;
  instructions: Array<TransactionInstruction>;
  recentBlockhash: Blockhash;
  addressLookupTableAccounts?: Array<AddressLookupTableAccount>;
};

export type GetAccountKeysArgs =
  | {
      accountKeysFromLookups?: AccountKeysFromLookups | null;
    }
  | {
      addressLookupTableAccounts?: AddressLookupTableAccount[] | null;
    };

export class MessageV0 {
  header: MessageHeader;
  staticAccountKeys: Array<PublicKey>;
  recentBlockhash: Blockhash;
  compiledInstructions: Array<MessageCompiledInstruction>;
  addressTableLookups: Array<MessageAddressTableLookup>;

  constructor(args: MessageV0Args) {
    this.header = args.header;
    this.staticAccountKeys = args.staticAccountKeys;
    this.recentBlockhash = args.recentBlockhash;
    this.compiledInstructions = args.compiledInstructions;
    this.addressTableLookups = args.addressTableLookups;
  }

  get version(): 0 {
    return 0;
  }

  get numAccountKeysFromLookups(): number {
    let count = 0;
    for (const lookup of this.addressTableLookups) {
      count += lookup.readonlyIndexes.length + lookup.writableIndexes.length;
    }
    return count;
  }

  getAccountKeys(args?: GetAccountKeysArgs): MessageAccountKeys {
    let accountKeysFromLookups: AccountKeysFromLookups | undefined;
    if (
      args &&
      'accountKeysFromLookups' in args &&
      args.accountKeysFromLookups
    ) {
      if (
        this.numAccountKeysFromLookups !=
        args.accountKeysFromLookups.writable.length +
          args.accountKeysFromLookups.readonly.length
      ) {
        throw new Error(
          'Failed to get account keys because of a mismatch in the number of account keys from lookups',
        );
      }
      accountKeysFromLookups = args.accountKeysFromLookups;
    } else if (
      args &&
      'addressLookupTableAccounts' in args &&
      args.addressLookupTableAccounts
    ) {
      accountKeysFromLookups = this.resolveAddressTableLookups(
        args.addressLookupTableAccounts,
      );
    } else if (this.addressTableLookups.length > 0) {
      throw new Error(
        'Failed to get account keys because address table lookups were not resolved',
      );
    }
    return new MessageAccountKeys(
      this.staticAccountKeys,
      accountKeysFromLookups,
    );
  }

  isAccountSigner(index: number): boolean {
    return index < this.header.numRequiredSignatures;
  }

  isAccountWritable(index: number): boolean {
    const numSignedAccounts = this.header.numRequiredSignatures;
    const numStaticAccountKeys = this.staticAccountKeys.length;
    if (index >= numStaticAccountKeys) {
      const lookupAccountKeysIndex = index - numStaticAccountKeys;
      const numWritableLookupAccountKeys = this.addressTableLookups.reduce(
        (count, lookup) => count + lookup.writableIndexes.length,
        0,
      );
      return lookupAccountKeysIndex < numWritableLookupAccountKeys;
    } else if (index >= this.header.numRequiredSignatures) {
      const unsignedAccountIndex = index - numSignedAccounts;
      const numUnsignedAccounts = numStaticAccountKeys - numSignedAccounts;
      const numWritableUnsignedAccounts =
        numUnsignedAccounts - this.header.numReadonlyUnsignedAccounts;
      return unsignedAccountIndex < numWritableUnsignedAccounts;
    } else {
      const numWritableSignedAccounts =
        numSignedAccounts - this.header.numReadonlySignedAccounts;
      return index < numWritableSignedAccounts;
    }
  }

  resolveAddressTableLookups(
    addressLookupTableAccounts: AddressLookupTableAccount[],
  ): AccountKeysFromLookups {
    const accountKeysFromLookups: AccountKeysFromLookups = {
      writable: [],
      readonly: [],
    };

    for (const tableLookup of this.addressTableLookups) {
      const tableAccount = addressLookupTableAccounts.find(account =>
        account.key.equals(tableLookup.accountKey),
      );
      if (!tableAccount) {
        throw new Error(
          `Failed to find address lookup table account for table key ${tableLookup.accountKey.toBase58()}`,
        );
      }

      for (const index of tableLookup.writableIndexes) {
        if (index < tableAccount.state.addresses.length) {
          accountKeysFromLookups.writable.push(
            tableAccount.state.addresses[index],
          );
        } else {
          throw new Error(
            `Failed to find address for index ${index} in address lookup table ${tableLookup.accountKey.toBase58()}`,
          );
        }
      }

      for (const index of tableLookup.readonlyIndexes) {
        if (index < tableAccount.state.addresses.length) {
          accountKeysFromLookups.readonly.push(
            tableAccount.state.addresses[index],
          );
        } else {
          throw new Error(
            `Failed to find address for index ${index} in address lookup table ${tableLookup.accountKey.toBase58()}`,
          );
        }
      }
    }

    return accountKeysFromLookups;
  }

  static compile(args: CompileV0Args): MessageV0 {
    const compiledKeys = CompiledKeys.compile(args.instructions, args.payerKey);

    const addressTableLookups = new Array<MessageAddressTableLookup>();
    const accountKeysFromLookups: AccountKeysFromLookups = {
      writable: new Array(),
      readonly: new Array(),
    };
    const lookupTableAccounts = args.addressLookupTableAccounts || [];
    for (const lookupTable of lookupTableAccounts) {
      const extractResult = compiledKeys.extractTableLookup(lookupTable);
      if (extractResult !== undefined) {
        const [addressTableLookup, {writable, readonly}] = extractResult;
        addressTableLookups.push(addressTableLookup);
        accountKeysFromLookups.writable.push(...writable);
        accountKeysFromLookups.readonly.push(...readonly);
      }
    }

    const [header, staticAccountKeys] = compiledKeys.getMessageComponents();
    const accountKeys = new MessageAccountKeys(
      staticAccountKeys,
      accountKeysFromLookups,
    );
    const compiledInstructions = accountKeys.compileInstructions(
      args.instructions,
    );
    return new MessageV0({
      header,
      staticAccountKeys,
      recentBlockhash: args.recentBlockhash,
      compiledInstructions,
      addressTableLookups,
    });
  }

  serialize(): Uint8Array {
    const encodedStaticAccountKeysLength = Array<number>();
    shortvec.encodeLength(
      encodedStaticAccountKeysLength,
      this.staticAccountKeys.length,
    );

    const serializedInstructions = this.serializeInstructions();
    const encodedInstructionsLength = Array<number>();
    shortvec.encodeLength(
      encodedInstructionsLength,
      this.compiledInstructions.length,
    );

    const serializedAddressTableLookups = this.serializeAddressTableLookups();
    const encodedAddressTableLookupsLength = Array<number>();
    shortvec.encodeLength(
      encodedAddressTableLookupsLength,
      this.addressTableLookups.length,
    );

    const messageLayout = BufferLayout.struct<{
      prefix: number;
      header: MessageHeader;
      staticAccountKeysLength: Uint8Array;
      staticAccountKeys: Array<Uint8Array>;
      recentBlockhash: Uint8Array;
      instructionsLength: Uint8Array;
      serializedInstructions: Uint8Array;
      addressTableLookupsLength: Uint8Array;
      serializedAddressTableLookups: Uint8Array;
    }>([
      BufferLayout.u8('prefix'),
      BufferLayout.struct<MessageHeader>(
        [
          BufferLayout.u8('numRequiredSignatures'),
          BufferLayout.u8('numReadonlySignedAccounts'),
          BufferLayout.u8('numReadonlyUnsignedAccounts'),
        ],
        'header',
      ),
      BufferLayout.blob(
        encodedStaticAccountKeysLength.length,
        'staticAccountKeysLength',
      ),
      BufferLayout.seq(
        Layout.publicKey(),
        this.staticAccountKeys.length,
        'staticAccountKeys',
      ),
      Layout.publicKey('recentBlockhash'),
      BufferLayout.blob(encodedInstructionsLength.length, 'instructionsLength'),
      BufferLayout.blob(
        serializedInstructions.length,
        'serializedInstructions',
      ),
      BufferLayout.blob(
        encodedAddressTableLookupsLength.length,
        'addressTableLookupsLength',
      ),
      BufferLayout.blob(
        serializedAddressTableLookups.length,
        'serializedAddressTableLookups',
      ),
    ]);

    const serializedMessage = new Uint8Array(PACKET_DATA_SIZE);
    const MESSAGE_VERSION_0_PREFIX = 1 << 7;
    const serializedMessageLength = messageLayout.encode(
      {
        prefix: MESSAGE_VERSION_0_PREFIX,
        header: this.header,
        staticAccountKeysLength: new Uint8Array(encodedStaticAccountKeysLength),
        staticAccountKeys: this.staticAccountKeys.map(key => key.toBytes()),
        recentBlockhash: bs58.decode(this.recentBlockhash),
        instructionsLength: new Uint8Array(encodedInstructionsLength),
        serializedInstructions,
        addressTableLookupsLength: new Uint8Array(
          encodedAddressTableLookupsLength,
        ),
        serializedAddressTableLookups,
      },
      serializedMessage,
    );
    return serializedMessage.slice(0, serializedMessageLength);
  }

  private serializeInstructions(): Uint8Array {
    let serializedLength = 0;
    const serializedInstructions = new Uint8Array(PACKET_DATA_SIZE);
    for (const instruction of this.compiledInstructions) {
      const encodedAccountKeyIndexesLength = Array<number>();
      shortvec.encodeLength(
        encodedAccountKeyIndexesLength,
        instruction.accountKeyIndexes.length,
      );

      const encodedDataLength = Array<number>();
      shortvec.encodeLength(encodedDataLength, instruction.data.length);

      const instructionLayout = BufferLayout.struct<{
        programIdIndex: number;
        encodedAccountKeyIndexesLength: Uint8Array;
        accountKeyIndexes: number[];
        encodedDataLength: Uint8Array;
        data: Uint8Array;
      }>([
        BufferLayout.u8('programIdIndex'),
        BufferLayout.blob(
          encodedAccountKeyIndexesLength.length,
          'encodedAccountKeyIndexesLength',
        ),
        BufferLayout.seq(
          BufferLayout.u8(),
          instruction.accountKeyIndexes.length,
          'accountKeyIndexes',
        ),
        BufferLayout.blob(encodedDataLength.length, 'encodedDataLength'),
        BufferLayout.blob(instruction.data.length, 'data'),
      ]);

      serializedLength += instructionLayout.encode(
        {
          programIdIndex: instruction.programIdIndex,
          encodedAccountKeyIndexesLength: new Uint8Array(
            encodedAccountKeyIndexesLength,
          ),
          accountKeyIndexes: instruction.accountKeyIndexes,
          encodedDataLength: new Uint8Array(encodedDataLength),
          data: instruction.data,
        },
        serializedInstructions,
        serializedLength,
      );
    }

    return serializedInstructions.slice(0, serializedLength);
  }

  private serializeAddressTableLookups(): Uint8Array {
    let serializedLength = 0;
    const serializedAddressTableLookups = new Uint8Array(PACKET_DATA_SIZE);
    for (const lookup of this.addressTableLookups) {
      const encodedWritableIndexesLength = Array<number>();
      shortvec.encodeLength(
        encodedWritableIndexesLength,
        lookup.writableIndexes.length,
      );

      const encodedReadonlyIndexesLength = Array<number>();
      shortvec.encodeLength(
        encodedReadonlyIndexesLength,
        lookup.readonlyIndexes.length,
      );

      const addressTableLookupLayout = BufferLayout.struct<{
        accountKey: Uint8Array;
        encodedWritableIndexesLength: Uint8Array;
        writableIndexes: number[];
        encodedReadonlyIndexesLength: Uint8Array;
        readonlyIndexes: number[];
      }>([
        Layout.publicKey('accountKey'),
        BufferLayout.blob(
          encodedWritableIndexesLength.length,
          'encodedWritableIndexesLength',
        ),
        BufferLayout.seq(
          BufferLayout.u8(),
          lookup.writableIndexes.length,
          'writableIndexes',
        ),
        BufferLayout.blob(
          encodedReadonlyIndexesLength.length,
          'encodedReadonlyIndexesLength',
        ),
        BufferLayout.seq(
          BufferLayout.u8(),
          lookup.readonlyIndexes.length,
          'readonlyIndexes',
        ),
      ]);

      serializedLength += addressTableLookupLayout.encode(
        {
          accountKey: lookup.accountKey.toBytes(),
          encodedWritableIndexesLength: new Uint8Array(
            encodedWritableIndexesLength,
          ),
          writableIndexes: lookup.writableIndexes,
          encodedReadonlyIndexesLength: new Uint8Array(
            encodedReadonlyIndexesLength,
          ),
          readonlyIndexes: lookup.readonlyIndexes,
        },
        serializedAddressTableLookups,
        serializedLength,
      );
    }

    return serializedAddressTableLookups.slice(0, serializedLength);
  }

  static deserialize(serializedMessage: Uint8Array): MessageV0 {
    let byteArray = [...serializedMessage];

    const prefix = byteArray.shift() as number;
    const maskedPrefix = prefix & VERSION_PREFIX_MASK;
    assert(
      prefix !== maskedPrefix,
      `Expected versioned message but received legacy message`,
    );

    const version = maskedPrefix;
    assert(
      version === 0,
      `Expected versioned message with version 0 but found version ${version}`,
    );

    const header: MessageHeader = {
      numRequiredSignatures: byteArray.shift() as number,
      numReadonlySignedAccounts: byteArray.shift() as number,
      numReadonlyUnsignedAccounts: byteArray.shift() as number,
    };

    const staticAccountKeys = [];
    const staticAccountKeysLength = shortvec.decodeLength(byteArray);
    for (let i = 0; i < staticAccountKeysLength; i++) {
      staticAccountKeys.push(
        new PublicKey(byteArray.splice(0, PUBLIC_KEY_LENGTH)),
      );
    }

    const recentBlockhash = bs58.encode(byteArray.splice(0, PUBLIC_KEY_LENGTH));

    const instructionCount = shortvec.decodeLength(byteArray);
    const compiledInstructions: MessageCompiledInstruction[] = [];
    for (let i = 0; i < instructionCount; i++) {
      const programIdIndex = byteArray.shift() as number;
      const accountKeyIndexesLength = shortvec.decodeLength(byteArray);
      const accountKeyIndexes = byteArray.splice(0, accountKeyIndexesLength);
      const dataLength = shortvec.decodeLength(byteArray);
      const data = new Uint8Array(byteArray.splice(0, dataLength));
      compiledInstructions.push({
        programIdIndex,
        accountKeyIndexes,
        data,
      });
    }

    const addressTableLookupsCount = shortvec.decodeLength(byteArray);
    const addressTableLookups: MessageAddressTableLookup[] = [];
    for (let i = 0; i < addressTableLookupsCount; i++) {
      const accountKey = new PublicKey(byteArray.splice(0, PUBLIC_KEY_LENGTH));
      const writableIndexesLength = shortvec.decodeLength(byteArray);
      const writableIndexes = byteArray.splice(0, writableIndexesLength);
      const readonlyIndexesLength = shortvec.decodeLength(byteArray);
      const readonlyIndexes = byteArray.splice(0, readonlyIndexesLength);
      addressTableLookups.push({
        accountKey,
        writableIndexes,
        readonlyIndexes,
      });
    }

    return new MessageV0({
      header,
      staticAccountKeys,
      recentBlockhash,
      compiledInstructions,
      addressTableLookups,
    });
  }
}
