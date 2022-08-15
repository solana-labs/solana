import {Blockhash} from '../blockhash';
import {
  MessageAddressTableLookup,
  MessageCompiledInstruction,
  MessageHeader,
} from './index';
import {Message} from './legacy';
import {PublicKey} from '../publickey';
import {MessageV0} from './v0';
import {TransactionVersion} from '../transaction/versioned';

/**
 * Common interface for versioned messages and legacy messages
 */
export interface VersionedMessage {
  get version(): TransactionVersion;
  get header(): MessageHeader;
  get staticAccountKeys(): Array<PublicKey>;
  get recentBlockhash(): Blockhash;
  get compiledInstructions(): Array<MessageCompiledInstruction>;
  get addressTableLookups(): Array<MessageAddressTableLookup>;
  serialize(): Uint8Array;
}

export function deserializeMessage(
  serializedMessage: Uint8Array,
): VersionedMessage {
  const prefix = serializedMessage[0];
  const MESSAGE_VERSION_PREFIX_MASK = 0x7f;
  const maskedPrefix = prefix & MESSAGE_VERSION_PREFIX_MASK;

  // if the highest bit of the prefix is not set, the message is not versioned
  if (maskedPrefix === prefix) {
    return Message.from(serializedMessage);
  }

  // the lower 7 bits of the prefix indicate the message version
  const version = maskedPrefix;
  if (version === 0) {
    return MessageV0.deserialize(serializedMessage);
  } else {
    throw new Error(
      `Transaction message version ${version} deserialization is not supported`,
    );
  }
}
