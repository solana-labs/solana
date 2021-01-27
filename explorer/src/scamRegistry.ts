const scamAddresses = toHash(["GACpXND1SSfTSQMmqGuFvGwXB3jGEYBDRGNzmLfTYwSP"]);

export function isScamAccount(address: string) {
  return address in scamAddresses;
}

function toHash(addresses: string[]) {
  return addresses.reduce((prev: { [addr: string]: boolean }, addr) => {
    prev[addr] = true;
    return prev;
  }, {});
}
