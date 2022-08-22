import {VERSION_PREFIX_MASK} from '../transaction/constants';
import {Message} from './legacy';
import {MessageV0} from './v0';

export type VersionedMessage = Message | MessageV0;
// eslint-disable-next-line no-redeclare
export const VersionedMessage = {
  deserialize: (serializedMessage: Uint8Array): VersionedMessage => {
    const prefix = serializedMessage[0];
    const maskedPrefix = prefix & VERSION_PREFIX_MASK;

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
  },
};
