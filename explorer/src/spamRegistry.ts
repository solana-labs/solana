const spamAddresses = toHash(["GACpXND1SSfTSQMmqGuFvGwXB3jGEYBDRGNzmLfTYwSP"]);

export function isSpamAccount(address: string) {
  return address in spamAddresses;
}

function toHash(addresses: string[]) {
  return addresses.reduce((prev: { [addr: string]: boolean }, addr) => {
    prev[addr] = true;
    return prev;
  }, {});
}
