const URL_RE = /^[^:]+:\/\/([^:[]+|\[[^\]]+\])(:\d+)?(.*)/i;

export function makeWebsocketUrl(endpoint: string) {
  const matches = endpoint.match(URL_RE);
  if (matches == null) {
    throw TypeError(`Failed to validate endpoint URL \`${endpoint}\``);
  }
  const [
    _, // eslint-disable-line @typescript-eslint/no-unused-vars
    hostish,
    portWithColon,
    rest,
  ] = matches;
  const protocol = endpoint.startsWith('https:') ? 'wss:' : 'ws:';
  const startPort =
    portWithColon == null ? null : parseInt(portWithColon.slice(1), 10);
  const websocketPort =
    // Only shift the port by +1 as a convention for ws(s) only if given endpoint
    // is explictly specifying the endpoint port (HTTP-based RPC), assuming
    // we're directly trying to connect to solana-validator's ws listening port.
    // When the endpoint omits the port, we're connecting to the protocol
    // default ports: http(80) or https(443) and it's assumed we're behind a reverse
    // proxy which manages WebSocket upgrade and backend port redirection.
    startPort == null ? '' : `:${startPort + 1}`;
  return `${protocol}//${hostish}${websocketPort}${rest}`;
}
