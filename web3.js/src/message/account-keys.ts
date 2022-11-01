import {LoadedAddresses} from '../connection';
import {PublicKey} from '../publickey';
import {TransactionInstruction} from '../transaction';
import {MessageCompiledInstruction} from './index';

export type AccountKeysFromLookups = LoadedAddresses;

export class MessageAccountKeys {
  staticAccountKeys: Array<PublicKey>;
  accountKeysFromLookups?: AccountKeysFromLookups;

  constructor(
    staticAccountKeys: Array<PublicKey>,
    accountKeysFromLookups?: AccountKeysFromLookups,
  ) {
    this.staticAccountKeys = staticAccountKeys;
    this.accountKeysFromLookups = accountKeysFromLookups;
  }

  keySegments(): Array<Array<PublicKey>> {
    const keySegments = [this.staticAccountKeys];
    if (this.accountKeysFromLookups) {
      keySegments.push(this.accountKeysFromLookups.writable);
      keySegments.push(this.accountKeysFromLookups.readonly);
    }
    return keySegments;
  }

  get(index: number): PublicKey | undefined {
    for (const keySegment of this.keySegments()) {
      if (index < keySegment.length) {
        return keySegment[index];
      } else {
        index -= keySegment.length;
      }
    }
    return;
  }

  get length(): number {
    return this.keySegments().flat().length;
  }

  compileInstructions(
    instructions: Array<TransactionInstruction>,
  ): Array<MessageCompiledInstruction> {
    // Bail early if any account indexes would overflow a u8
    const U8_MAX = 255;
    if (this.length > U8_MAX + 1) {
      throw new Error('Account index overflow encountered during compilation');
    }

    const keyIndexMap = new Map();
    this.keySegments()
      .flat()
      .forEach((key, index) => {
        keyIndexMap.set(key.toBase58(), index);
      });

    const findKeyIndex = (key: PublicKey) => {
      const keyIndex = keyIndexMap.get(key.toBase58());
      if (keyIndex === undefined)
        throw new Error(
          'Encountered an unknown instruction account key during compilation',
        );
      return keyIndex;
    };

    return instructions.map((instruction): MessageCompiledInstruction => {
      return {
        programIdIndex: findKeyIndex(instruction.programId),
        accountKeyIndexes: instruction.keys.map(meta =>
          findKeyIndex(meta.pubkey),
        ),
        data: instruction.data,
      };
    });
  }
}
