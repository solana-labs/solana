// flow-typed signature: 800c99f4687ac083d3ed2dd6b9ee9457
// flow-typed version: 711a5f887a/node-fetch_v2.x.x/flow_>=v0.104.x

declare module 'node-fetch' {
  import type http from 'http';
  import type https from 'https';
  import type { URL } from 'url';
  import type { Readable } from 'stream';

  declare export type AbortSignal = {
    +aborted: boolean;
    +onabort: (event?: { ... }) => void;

    +addEventListener: (name: string, cb: () => mixed) => void;
    +removeEventListener: (name: string, cb: () => mixed) => void;
    +dispatchEvent: (event: { ... }) => void;
    ...,
  }

  declare export class Request mixins Body {
    constructor(input: string | { href: string, ... } | Request, init?: RequestInit): this;
    context: RequestContext;
    headers: Headers;
    method: string;
    redirect: RequestRedirect;
    referrer: string;
    url: string;

    // node-fetch extensions
    agent: http.Agent | https.Agent;
    compress: boolean;
    counter: number;
    follow: number;
    hostname: string;
    port: number;
    protocol: string;
    size: number;
    timeout: number;
  }

  declare type HeaderObject = { [index: string]: string, ... }

  declare type RequestInit = {|
    body?: BodyInit,
    headers?: HeaderObject,
    method?: string,
    redirect?: RequestRedirect,
    signal?: AbortSignal | null,

    // node-fetch extensions
    agent?: (URL => (http.Agent | https.Agent)) | http.Agent | https.Agent | null;
    compress?: boolean,
    follow?: number,
    size?: number,
    timeout?: number,
  |};

  declare export interface FetchError extends Error {
    name: 'FetchError';
    type: string;
    code: ?number;
    errno: ?number;
  }

  declare export interface AbortError extends Error {
    name: 'AbortError';
    type: 'aborted';
  }

  declare type RequestContext =
    'audio' | 'beacon' | 'cspreport' | 'download' | 'embed' |
    'eventsource' | 'favicon' | 'fetch' | 'font' | 'form' | 'frame' |
    'hyperlink' | 'iframe' | 'image' | 'imageset' | 'import' |
    'internal' | 'location' | 'manifest' | 'object' | 'ping' | 'plugin' |
    'prefetch' | 'script' | 'serviceworker' | 'sharedworker' |
    'subresource' | 'style' | 'track' | 'video' | 'worker' |
    'xmlhttprequest' | 'xslt';
  declare type RequestRedirect = 'error' | 'follow' | 'manual';

  declare export class Headers {
    append(name: string, value: string): void;
    delete(name: string): void;
    forEach(callback: (value: string, name: string) => void): void;
    get(name: string): string;
    getAll(name: string): Array<string>;
    has(name: string): boolean;
    raw(): { [k: string]: string[], ... };
    set(name: string, value: string): void;
  }

  declare export class Body {
    buffer(): Promise<Buffer>;
    json(): Promise<any>;
    json<T>(): Promise<T>;
    text(): Promise<string>;
    body: stream$Readable;
    bodyUsed: boolean;
  }

  declare export class Response mixins Body {
    constructor(body?: BodyInit, init?: ResponseInit): this;
    clone(): Response;
    error(): Response;
    redirect(url: string, status: number): Response;
    headers: Headers;
    ok: boolean;
    status: number;
    statusText: string;
    size: number;
    timeout: number;
    type: ResponseType;
    url: string;
  }

  declare type ResponseType =
    | 'basic'
    | 'cors'
    | 'default'
    | 'error'
    | 'opaque'
    | 'opaqueredirect';

  declare interface ResponseInit {
    headers?: HeaderInit,
    status: number,
    statusText?: string,
  }

  declare type HeaderInit = Headers | Array<string>;
  declare type BodyInit = string | null | Buffer | Blob | Readable;

  declare export default function fetch(url: string | Request, init?: RequestInit): Promise<Response>
}
