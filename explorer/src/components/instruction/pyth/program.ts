import * as BufferLayout from "@solana/buffer-layout";
import {
  InstructionType,
  PublicKey,
  TransactionInstruction,
} from "@solana/web3.js";
import { Layout, uint8ArrayToBuffer } from "@solana/buffer-layout";

/**
 * An enumeration of valid PythInstructionTypes
 */
export type PythInstructionType =
  | "InitMapping"
  | "AddMapping"
  | "AddProduct"
  | "UpdateProduct"
  | "AddPrice"
  | "AddPublisher"
  | "DeletePublisher"
  | "UpdatePrice"
  | "AggregatePrice"
  | "InitPrice"
  | "InitTest"
  | "UpdateTest"
  | "SetMinPublishers"
  | "UpdatePriceNoFailOnError";

export function headerLayout(property: string = "header") {
  return BufferLayout.struct(
    [BufferLayout.u32("version"), BufferLayout.u32("type")],
    property
  );
}

function decodeData(type: InstructionType, buffer: Buffer): any {
  let data;
  try {
    data = type.layout.decode(buffer);
  } catch (err) {
    throw new Error("invalid instruction; " + err);
  }

  if (data.header.type !== type.index) {
    throw new Error(
      `invalid instruction; instruction index mismatch ${data.header.type} != ${type.index}`
    );
  }

  return data;
}

/**
 * An uint8 length-prefixed UTF-8 string.
 */
class LPString extends Layout {
  getSpan(b: Uint8Array, offset?: number): number {
    return 1 + b[offset || 0];
  }

  decode(b: Uint8Array, offset?: number): string {
    if (offset === undefined) {
      offset = 0;
    }
    return uint8ArrayToBuffer(b)
      .slice(offset + 1, offset + b[offset] + 1)
      .toString("utf-8");
  }
}

/**
 * A list that fills up all the available space with its elements.
 */
class GreedyList extends Layout {
  private element: Layout;

  constructor(element: Layout, property?: string) {
    super(-1, property);
    this.element = element;
  }

  getSpan(b: Uint8Array, offset?: number): number {
    return b.length - (offset || 0);
  }

  decode(b: Uint8Array, offset?: number): string[] {
    if (offset === undefined) {
      offset = 0;
    }
    const strs = [];
    while (offset < b.length) {
      strs.push(this.element.decode(b, offset));
      offset += this.element.getSpan(b, offset);
    }
    return strs;
  }
}

/**
 * An enumeration of valid Pyth instruction layouts
 * @internal
 */
export const PYTH_INSTRUCTION_LAYOUTS: {
  [type in PythInstructionType]: InstructionType;
} = Object.freeze({
  InitMapping: {
    index: 0,
    layout: BufferLayout.struct([headerLayout()]),
  },
  AddMapping: {
    index: 1,
    layout: BufferLayout.struct([headerLayout()]),
  },
  AddProduct: {
    index: 2,
    layout: BufferLayout.struct([headerLayout()]),
  },
  UpdateProduct: {
    index: 3,
    layout: BufferLayout.struct([
      headerLayout(),
      new GreedyList(
        BufferLayout.struct([
          new LPString(-1, "key"),
          new LPString(-1, "value"),
        ]),
        "attributes"
      ),
    ]),
  },
  AddPrice: {
    index: 4,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.s32("exponent"),
      BufferLayout.u32("priceType"),
    ]),
  },
  AddPublisher: {
    index: 5,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.blob(32, "publisherPubkey"),
    ]),
  },
  DeletePublisher: {
    index: 6,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.blob(32, "publisherPubkey"),
    ]),
  },
  UpdatePrice: {
    index: 7,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.u32("status"),
      BufferLayout.u32("unused1"),
      BufferLayout.ns64("price"),
      BufferLayout.nu64("conf"),
      BufferLayout.nu64("publishSlot"),
    ]),
  },
  AggregatePrice: {
    index: 8,
    layout: BufferLayout.struct([headerLayout()]),
  },
  InitPrice: {
    index: 9,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.s32("exponent"),
      BufferLayout.u32("priceType"),
    ]),
  },
  InitTest: {
    index: 10,
    layout: BufferLayout.struct([headerLayout()]),
  },
  UpdateTest: {
    index: 11,
    layout: BufferLayout.struct([headerLayout()]),
  },
  SetMinPublishers: {
    index: 12,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.u8("minPublishers"),
      BufferLayout.blob(3, "unused1"),
    ]),
  },
  UpdatePriceNoFailOnError: {
    index: 13,
    layout: BufferLayout.struct([
      headerLayout(),
      BufferLayout.u32("status"),
      BufferLayout.u32("unused1"),
      BufferLayout.ns64("price"),
      BufferLayout.nu64("conf"),
      BufferLayout.nu64("publishSlot"),
    ]),
  },
});

export enum PriceType {
  Unknown = 0,
  Price,
}

export enum TradingStatus {
  Unknown = 0,
  Trading,
  Halted,
  Auction,
}

export type InitMappingParams = {
  fundingPubkey: PublicKey;
  mappingPubkey: PublicKey;
};

export type AddMappingParams = {
  fundingPubkey: PublicKey;
  mappingPubkey: PublicKey;
  nextMappingPubkey: PublicKey;
};

export type AddProductParams = {
  fundingPubkey: PublicKey;
  mappingPubkey: PublicKey;
  productPubkey: PublicKey;
};

export type UpdateProductParams = {
  fundingPubkey: PublicKey;
  productPubkey: PublicKey;
  attributes: Map<String, String>;
};

export type AddPriceParams = {
  fundingPubkey: PublicKey;
  productPubkey: PublicKey;
  pricePubkey: PublicKey;
  exponent: number;
  priceType: PriceType;
};

export type BasePublisherOperationParams = {
  signerPubkey: PublicKey;
  pricePubkey: PublicKey;
  publisherPubkey: PublicKey;
};

export type UpdatePriceParams = {
  publisherPubkey: PublicKey;
  pricePubkey: PublicKey;
  status: TradingStatus;
  price: number;
  conf: number;
  publishSlot: number;
};

export type AggregatePriceParams = {
  fundingPubkey: PublicKey;
  pricePubkey: PublicKey;
};

export type InitPriceParams = {
  fundingPubkey: PublicKey;
  pricePubkey: PublicKey;
  exponent: number;
  priceType: PriceType;
};

export type SetMinPublishersParams = {
  fundingPubkey: PublicKey;
  pricePubkey: PublicKey;
  minPublishers: number;
};

/**
 * Pyth Instruction class
 */
export class PythInstruction {
  /**
   * Decode a Pyth instruction and retrieve the instruction type.
   */
  static decodeInstructionType(
    instruction: TransactionInstruction
  ): PythInstructionType {
    const header = headerLayout().decode(instruction.data);
    if (header.version !== 2) {
      throw new Error(`Unsupported Pyth version: ${header.version}`);
    }
    const typeIndex = header.type;

    let type: PythInstructionType | undefined;
    for (const [ixType, layout] of Object.entries(PYTH_INSTRUCTION_LAYOUTS)) {
      if (layout.index === typeIndex) {
        type = ixType as PythInstructionType;
        break;
      }
    }

    if (!type) {
      throw new Error("Instruction type incorrect; not a PythInstruction");
    }

    return type;
  }

  /**
   * Decode an "init mapping" instruction and retrieve the instruction params.
   */
  static decodeInitMapping(
    instruction: TransactionInstruction
  ): InitMappingParams {
    decodeData(PYTH_INSTRUCTION_LAYOUTS.InitMapping, instruction.data);
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      mappingPubkey: instruction.keys[1].pubkey,
    };
  }

  /**
   * Decode an "add mapping" instruction and retrieve the instruction params.
   */
  static decodeAddMapping(
    instruction: TransactionInstruction
  ): AddMappingParams {
    decodeData(PYTH_INSTRUCTION_LAYOUTS.AddMapping, instruction.data);
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      mappingPubkey: instruction.keys[1].pubkey,
      nextMappingPubkey: instruction.keys[2].pubkey,
    };
  }

  /**
   * Decode an "add product" instruction and retrieve the instruction params.
   */
  static decodeAddProduct(
    instruction: TransactionInstruction
  ): AddProductParams {
    decodeData(PYTH_INSTRUCTION_LAYOUTS.AddProduct, instruction.data);
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      mappingPubkey: instruction.keys[1].pubkey,
      productPubkey: instruction.keys[2].pubkey,
    };
  }

  /**
   * Decode an "add product" instruction and retrieve the instruction params.
   */
  static decodeUpdateProduct(
    instruction: TransactionInstruction
  ): UpdateProductParams {
    const { attributes } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.UpdateProduct,
      instruction.data
    );
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      productPubkey: instruction.keys[1].pubkey,
      attributes: new Map(
        attributes.map((kv: { key: string; value: string }) => [
          kv.key,
          kv.value,
        ])
      ),
    };
  }

  /**
   * Decode an "add price" instruction and retrieve the instruction params.
   */
  static decodeAddPrice(instruction: TransactionInstruction): AddPriceParams {
    const { exponent, priceType } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.AddPrice,
      instruction.data
    );
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      productPubkey: instruction.keys[1].pubkey,
      pricePubkey: instruction.keys[2].pubkey,
      exponent,
      priceType,
    };
  }

  /**
   * Decode an "add publisher" instruction and retrieve the instruction params.
   */
  static decodeAddPublisher(
    instruction: TransactionInstruction
  ): BasePublisherOperationParams {
    const { publisherPubkey } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.AddPublisher,
      instruction.data
    );

    return {
      signerPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      publisherPubkey: PublicKey.decode(publisherPubkey),
    };
  }

  /**
   * Decode an "delete publisher" instruction and retrieve the instruction params.
   */
  static decodeDeletePublisher(
    instruction: TransactionInstruction
  ): BasePublisherOperationParams {
    const { publisherPubkey } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.DeletePublisher,
      instruction.data
    );

    return {
      signerPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      publisherPubkey: PublicKey.decode(publisherPubkey),
    };
  }

  /**
   * Decode an "update price" instruction and retrieve the instruction params.
   */
  static decodeUpdatePrice(
    instruction: TransactionInstruction
  ): UpdatePriceParams {
    const { status, price, conf, publishSlot } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.UpdatePrice,
      instruction.data
    );

    return {
      publisherPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      status,
      price,
      conf,
      publishSlot,
    };
  }

  /**
   * Decode an "update price no fail error" instruction and retrieve the instruction params.
   */
  static decodeUpdatePriceNoFailOnError(
    instruction: TransactionInstruction
  ): UpdatePriceParams {
    const { status, price, conf, publishSlot } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.UpdatePriceNoFailOnError,
      instruction.data
    );

    return {
      publisherPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      status,
      price,
      conf,
      publishSlot,
    };
  }

  /**
   * Decode an "aggregate price" instruction and retrieve the instruction params.
   */
  static decodeAggregatePrice(
    instruction: TransactionInstruction
  ): AggregatePriceParams {
    decodeData(PYTH_INSTRUCTION_LAYOUTS.AggregatePrice, instruction.data);

    return {
      fundingPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
    };
  }

  /**
   * Decode an "init price" instruction and retrieve the instruction params.
   */
  static decodeInitPrice(instruction: TransactionInstruction): InitPriceParams {
    const { exponent, priceType } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.InitPrice,
      instruction.data
    );
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      exponent,
      priceType,
    };
  }

  /**
   * Decode an "set min publishers" instruction and retrieve the instruction params.
   */
  static decodeSetMinPublishers(
    instruction: TransactionInstruction
  ): SetMinPublishersParams {
    const { minPublishers } = decodeData(
      PYTH_INSTRUCTION_LAYOUTS.SetMinPublishers,
      instruction.data
    );
    return {
      fundingPubkey: instruction.keys[0].pubkey,
      pricePubkey: instruction.keys[1].pubkey,
      minPublishers,
    };
  }
}
