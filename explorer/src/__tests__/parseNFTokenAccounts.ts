import { PublicKey } from "@solana/web3.js";
import { expect } from "chai";
import { NFTOKEN_ADDRESS } from "../components/account/nftoken/nftoken";
import { parseNFTokenNFTAccount } from "../components/account/nftoken/isNFTokenAccount";

describe("parseNFTokenAccounts", () => {
  it("parses an NFT", () => {
    const buffer = new Uint8Array([
      33, 180, 91, 53, 236, 15, 63, 97, 1, 13, 194, 212, 59, 127, 163, 1, 184,
      232, 229, 196, 221, 132, 114, 202, 93, 251, 147, 255, 156, 194, 45, 162,
      89, 138, 54, 129, 145, 16, 170, 225, 110, 171, 80, 175, 146, 42, 195, 197,
      124, 142, 197, 32, 198, 20, 137, 26, 33, 27, 67, 163, 173, 127, 113, 232,
      108, 17, 2, 184, 52, 59, 71, 87, 97, 1, 178, 138, 249, 251, 68, 1, 82,
      163, 86, 56, 204, 21, 192, 126, 64, 94, 187, 81, 78, 188, 73, 85, 189,
      140, 52, 199, 206, 30, 238, 117, 158, 114, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 67, 0, 0, 0, 104, 116, 116, 112, 115, 58, 47, 47, 99, 100, 110, 46,
      103, 108, 111, 119, 46, 97, 112, 112, 47, 110, 47, 56, 56, 47, 55, 56,
      101, 102, 49, 55, 99, 49, 45, 50, 98, 53, 97, 45, 52, 54, 56, 101, 45, 97,
      101, 56, 102, 45, 55, 52, 48, 51, 56, 53, 54, 101, 57, 102, 48, 48, 46,
      106, 115, 111, 110, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    const nftAccount = parseNFTokenNFTAccount({
      pubkey: new PublicKey("FagABcRBhZH27JDtu6A1Jo9woXyoznP28QujLkxkN9Hj"),
      details: { rawData: buffer, owner: new PublicKey(NFTOKEN_ADDRESS) },
    } as any);
    expect(nftAccount!.metadata_url).to.eq(
      "https://cdn.glow.app/n/88/78ef17c1-2b5a-468e-ae8f-7403856e9f00.json"
    );
  });
});
