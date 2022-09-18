import * as BufferLayout from "@solana/buffer-layout";
import { Connection, PublicKey } from "@solana/web3.js";
import axios from "axios";

export const GLOW_ID_ADDRESS = "GLoW6kDXmQHFjtQQ44ccmXjosqrKkE54bVbUTA4bA3zs";

export const hasGlowIdSyntax = (value: string) => {
  return value.length > 6 && value.substring(value.length - 5) === ".glow";
};

export namespace GlowIdTypes {
  export type Info = {
    handle: string;
    name: string | null;
    image: string | null;
    metadata_url: string;
    twitter: string | null;
    resolved: PublicKey;
    joined_at: Date;
    controller: PublicKey;
    address: PublicKey;
  };
}

export async function getGlowIdFromHandle(
  query: string,
  connection: Connection
): Promise<GlowIdTypes.Info | null> {
  const [handle] = query.split(".");

  const handleBuffer = Buffer.alloc(16);
  handleBuffer.write(handle);

  const [handlePubkey] = PublicKey.findProgramAddressSync(
    [Buffer.from("handle@"), handleBuffer],
    new PublicKey(GLOW_ID_ADDRESS)
  );
  const handleAccount = await connection.getAccountInfo(handlePubkey);

  if (!handleAccount) {
    return null;
  }

  return parseGlowIdAccount({ data: handleAccount.data, handlePubkey });
}

const glowIdAccountDisc = "5QbX+qna3us=";

const arrToStr = (arr: Uint8Array | Buffer): string => {
  return Buffer.from(Array.from(arr).filter((byte) => byte !== 0)).toString();
};

export const parseGlowIdAccount = async ({
  data,
  handlePubkey,
}: {
  data: Buffer | null | undefined;
  handlePubkey: PublicKey;
}): Promise<GlowIdTypes.Info | null> => {
  if (!data) {
    return null;
  }

  const parsed = glowIdAccountLayout.decode(data);

  const disc = Buffer.from(parsed.discriminator).toString("base64");
  if (disc !== glowIdAccountDisc) {
    return null;
  }

  const metadata_url = parsed.metadata_url?.replace(/\0/g, "");
  const controller = new PublicKey(parsed.controller);
  const resolved = new PublicKey(parsed.resolved);
  const handle = arrToStr(parsed.handle);
  const joined_at = new Date(parsed.joined_at * 1000);

  try {
    const { data } = await axios.get(metadata_url);
    return {
      handle,
      name: data.name ?? null,
      image: data.image ?? null,
      metadata_url,
      twitter: arrToStr(parsed.twitter?.handle) ?? null,
      joined_at,
      resolved,
      controller,
      address: handlePubkey,
    };
  } catch {
    return {
      handle,
      name: null,
      joined_at,
      image: null,
      metadata_url,
      twitter: arrToStr(parsed.twitter?.handle) ?? null,
      resolved,
      controller,
      address: handlePubkey,
    };
  }
};

const publicKey = (property: string) => {
  return BufferLayout.blob(32, property);
};

const twitter = BufferLayout.union(
  BufferLayout.u8("has_twitter"),
  null,
  "twitter"
);
twitter.addVariant(0, BufferLayout.u8("no_twitter"), "no_twitter");
twitter.addVariant(1, BufferLayout.blob(16, "handle"), "handle");

const glowIdAccountLayout = BufferLayout.struct([
  BufferLayout.blob(8, "discriminator"),
  BufferLayout.u8("version"),
  BufferLayout.blob(16, "handle"),
  BufferLayout.blob(16, "invited_by_handle"),
  publicKey("resolved"),
  publicKey("controller"),
  BufferLayout.u8("bump"),
  BufferLayout.nu64("joined_at"),
  publicKey("nft_collection"),
  twitter, // Figure out how to do a coption
  BufferLayout.u32("metadata_url_length"),
  BufferLayout.utf8(400, "metadata_url"),
]);
