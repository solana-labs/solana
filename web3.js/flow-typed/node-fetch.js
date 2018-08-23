declare module 'node-fetch' {
  declare export type Config = {
    method?: string;
    headers?: {[key: string]: string};
    compress?: bool;
    body?: Buffer|string;
    size?: number;
  }

  declare export class Headers {
    get(name: string): ?string;
    set(name: string, value: string): void;
  }

  declare export class Response {
    url: string;
    status: string;
    statusText: string;
    headers: Headers;
    ok: bool;

    json(): Promise<Object>;
    text(): Promise<string>;
  }

  declare export default (url: string, config?: Config) => Promise<Response>;
}
