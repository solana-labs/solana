import { getDecentralizedURI } from "../utils/url";
import fetch from 'jest-fetch-mock';
import { act, renderHook } from '@testing-library/react-hooks'
import { useCachedImage } from "../components/common/hooks/useCachedImage";

describe('Tests the utility functions and hook for decentralized uri transformations', () => {
  beforeEach(() => {
    fetch.resetMocks();
  });

  it('should return http url as is when protocol is http', async () => {
    let fakeHttpURL = 'http://www.google.com/fakeimage'
    let result = await getDecentralizedURI(fakeHttpURL)
    expect(result).toBe(fakeHttpURL)
  })

  it('should return https url as is when protocol is https', async () => {
    let fakeHttpURL = 'https://www.google.com/fakeimage'
    let result = await getDecentralizedURI(fakeHttpURL)
    expect(result).toBe(fakeHttpURL)
  })

  it('should parse and fetch image if one exists for arweave url', async () => {
    fetch.mockResponseOnce(JSON.stringify({ "name": "ART #0000", "symbol": "", "description": "You hold in your possession an OG thugbird. It was created with love for the Solana community by 0x_thug", "seller_fee_basis_points": 500, "external_url": "https://www.thugbirdz.com/", "attributes": [{ "trait_type": "Background Color", "value": "palegreen" }, { "trait_type": "Head Color", "value": "lightblue" }, { "trait_type": "Neck Color", "value": "lightslategray" }], "collection": { "name": "Test Collection", "family": "thugbirdz" }, "properties": { "files": [{ "uri": "https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw", "type": "image/png" }], "category": "image", "maxSupply": 0, "creators": [{ "address": "CBBUMHRmbVUck99mTCip5sHP16kzGj3QTYB8K3XxwmQx", "share": 100 }] }, "image": "https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw" }));
    let arweaveURI = 'ar://eR4wgSnWusIG-xF2BZzsiOwVehQsvfCT8VAUC4NHQ5Y'
    let endURI = 'https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw'
    let result = await getDecentralizedURI(arweaveURI)
    expect(result).toBe(endURI)
  })

  it('should parse and return raw url if no arweave image exists', async () => {
    fetch.mockResponseOnce(JSON.stringify({ "name": "ART #0000", "symbol": "", "description": "You hold in your possession an OG thugbird. It was created with love for the Solana community by 0x_thug", "seller_fee_basis_points": 500, "external_url": "https://www.thugbirdz.com/", "attributes": [{ "trait_type": "Background Color", "value": "palegreen" }, { "trait_type": "Head Color", "value": "lightblue" }, { "trait_type": "Neck Color", "value": "lightslategray" }], "collection": { "name": "Test Collection", "family": "thugbirdz" }, "properties": { "files": [{ "uri": "https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw", "type": "image/png" }], "category": "image", "maxSupply": 0, "creators": [{ "address": "CBBUMHRmbVUck99mTCip5sHP16kzGj3QTYB8K3XxwmQx", "share": 100 }] } }));

    let arweaveURI = 'ar://eR4wgSnWusIG-xF2BZzsiOwVehQsvfCT8VAUC4NHQ5Y'
    let result = await getDecentralizedURI(arweaveURI)
    expect(result).toBe(arweaveURI)
  })

  it('should parse and return ipfs https url for ipfs protocol', async () => {
    let ipfsUrl = 'ipfs://bafkreigntnnko64hmhnfalo7rbht6q75mccqfjrrdv5givg5zcsepv3eku'
    let endURI = 'https://ipfs.io/ipfs/bafkreigntnnko64hmhnfalo7rbht6q75mccqfjrrdv5givg5zcsepv3eku'
    let result = await getDecentralizedURI(ipfsUrl)
    expect(result).toBe(endURI)
  })

  it('should not blow up if url is unkown protocol', async () => {
    let unkownProtocol = 'xxaa://bafkreigntnnko64hmhnfalo7rbht6q75mccqfjrrdv5givg5zcsepv3eku'
    let result = await getDecentralizedURI(unkownProtocol)
    expect(result).toBe(unkownProtocol)
  })

  it('should work without url, just returns empty for cached blob', async () => {
    const { result } = renderHook(({ url }) => useCachedImage(url), {
      initialProps: { url: '' }
    })
    expect(result.current.cachedBlob).toBe(undefined)

  })

  it('should work with airweave url, cached blob should have image', async () => {
    let arweaveURI = 'ar://eR4wgSnWusIG-xF2BZzsiOwVehQsvfCT8VAUC4NHQ5Y'
    let endURI = 'https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw'
    const fakeBlob = "fakeblobdata"
    fetch.mockOnce(
      JSON.stringify({ "name": "ART #0000", "symbol": "", "description": "You hold in your possession an OG thugbird. It was created with love for the Solana community by 0x_thug", "seller_fee_basis_points": 500, "external_url": "https://www.thugbirdz.com/", "attributes": [{ "trait_type": "Background Color", "value": "palegreen" }, { "trait_type": "Head Color", "value": "lightblue" }, { "trait_type": "Neck Color", "value": "lightslategray" }], "collection": { "name": "Test Collection", "family": "thugbirdz" }, "properties": { "files": [{ "uri": "https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw", "type": "image/png" }], "category": "image", "maxSupply": 0, "creators": [{ "address": "CBBUMHRmbVUck99mTCip5sHP16kzGj3QTYB8K3XxwmQx", "share": 100 }] }, "image": "https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw" })
    );
    fetchMock.mockIf('https://arweave.net/iRfKZbkkA5BR7R6sOBHav9nR1g-x85ZXjOfT7it7fJw', async () => {
      return {
        body: await new Blob([fakeBlob], { type: 'image/png' }).text(),
      }
    })

    const { result, waitForNextUpdate } = renderHook(({ url }) => useCachedImage(url), {
      initialProps: { url: arweaveURI }
    })

    await waitForNextUpdate()
    expect(result.current.cachedBlob).toBe(endURI)

  })
})