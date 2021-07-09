export function makeWebsocketUrl(endpoint: string) {
  let url = new URL(endpoint);
  const useHttps = url.protocol === 'https:';

  url.protocol = useHttps ? 'wss:' : 'ws:';
  url.host = '';

  // Only shift the port by +1 as a convention for ws(s) only if given endpoint
  // is explictly specifying the endpoint port (HTTP-based RPC), assuming
  // we're directly trying to connect to solana-validator's ws listening port.
  // When the endpoint omits the port, we're connecting to the protocol
  // default ports: http(80) or https(443) and it's assumed we're behind a reverse
  // proxy which manages WebSocket upgrade and backend port redirection.
  if (url.port !== '') {
    url.port = String(Number(url.port) + 1);
  }
  return url.toString();
}
