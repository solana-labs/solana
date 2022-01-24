import type {Buffer} from 'buffer';

import {CLUSTER_NAMES, Cluster, clusterApiUrl} from './cluster';
import {Message} from '../message';
import {Transaction} from '../transaction';

/**
 * Checks if all transactions should be logged
 * Returns true if one of the following is set
 *   env variable `SOLANA_TRANSACTION_LOG_LEVEL`
 *   `process.env.SOLANA_TRANSACTION_LOG_LEVEL`
 *   `window.SOLANA_TRANSACTION_LOG_LEVEL`
 */
export function checkLogAllTransactions(): boolean {
  return (
    (typeof window != 'undefined' && 'SOLANA_LOG_ALL_TRANSACTIONS' in window) ||
    process.env['SOLANA_LOG_ALL_TRANSACTIONS'] !== undefined
  );
}

export const DEFAULT_EXPLORER_BASE_URL = 'https://explorer.solana.com';
export let EXPLORER_BASE_URL = DEFAULT_EXPLORER_BASE_URL;

/**
 * Sets the base url for all inspect functions
 *
 * Defaults to `DEFAULT_EXPLORER_BASE_URL`
 *
 * @param {string} url
 */
export function setExplorerBaseUrl(url: string): void {
  EXPLORER_BASE_URL = url;
}

/**
 * Log a link to inspect message, defaults to console.log
 *
 * Link is of the form
 *   `https://explorer.solana.com/tx/inspector?message=<base64 encoded message>` for transaction messages
 *   `https://explorer.solana.com/tx/<signature>/inspect` for signatures
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {Buffer | Message | Transaction | string} inspectData
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @param {(...any) => void} log? = console.log
 */
export function inspectTransaction(
  inspectData: Buffer | Message | Transaction | string,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
  log: (...args: any[]) => void = console.log,
): void {
  log(getInspectTransactionUrl(inspectData, clusterOrUrl));
}

/**
 * Get a link to inspect the transaction message or signature
 *
 * Link is of the form
 *   `https://explorer.solana.com/tx/inspector?message=<base64 encoded message>` for transaction messages
 *   `https://explorer.solana.com/tx/<signature>/inspect` for signatures
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {Buffer | Message | Transaction | string} inspectData
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @returns {string}
 */
export function getInspectTransactionUrl(
  inspectData: Buffer | Message | Transaction | string,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
): string {
  if (typeof inspectData === 'string') {
    return getInspectSignatureUrl(inspectData, clusterOrUrl);
  } else {
    return getInspectMessageUrl(inspectData, clusterOrUrl);
  }
}

/**
 * Log a link to inspect the transaction message, defaults to console.log
 *
 * Link is of the form `https://explorer.solana.com/tx/inspector?message=<base64 encoded message>`
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {Buffer | Message | Transaction} inspectData
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @param {(...any) => void} log? = console.log
 */
export function inspectMessage(
  inspectData: Buffer | Message | Transaction,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
  log: (...args: any[]) => void = console.log,
): void {
  log(getInspectMessageUrl(inspectData, clusterOrUrl));
}

/**
 * Get a link to inspect the transaction message
 *
 * Link is of the form `https://explorer.solana.com/tx/inspector?message=<base64 encoded message>`
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {Buffer | Message | Transaction} inspectData
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @returns {string}
 */
export function getInspectMessageUrl(
  inspectData: Buffer | Message | Transaction,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
): string {
  const url = getExplorerUrlWithCluster('tx/inspector', clusterOrUrl);

  let message = inspectData;
  if (inspectData instanceof Message || inspectData instanceof Transaction) {
    message = inspectData.serialize();
  }

  url.searchParams.append('message', message.toString('base64'));

  return url.href;
}

/**
 * Log a link to inspect the transaction signature, defaults to console.log
 *
 * Link is of the form `https://explorer.solana.com/tx/<signature>/inspect`
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {string} signature
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @param {(...any) => void} log? = console.log
 */
export function inspectSignature(
  signature: string,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
  log: (...args: any[]) => void = console.log,
): void {
  log(getInspectSignatureUrl(signature, clusterOrUrl));
}

/**
 * Get a link to inspect the transaction signature
 *
 * Link is of the form `https://explorer.solana.com/tx/<signature>/inspect`
 * Respects `EXPLORER_BASE_URL`
 *
 * @param {string} signature
 * @param {Cluster | string} cluster? = "mainnet-beta"
 * @returns {string}
 */
export function getInspectSignatureUrl(
  signature: string,
  clusterOrUrl: Cluster | string = 'mainnet-beta',
): string {
  const url = getExplorerUrlWithCluster(
    `tx/${signature}/inspect`,
    clusterOrUrl,
  );
  return url.href;
}

/**
 * @internal
 */
export function getExplorerUrlWithCluster(
  path: string,
  clusterOrUrl: Cluster | string,
): URL {
  const url = new URL(path, EXPLORER_BASE_URL);

  for (let cluster of CLUSTER_NAMES)
    if (clusterApiUrl(cluster, true) == clusterOrUrl) {
      clusterOrUrl = cluster;
      break;
    }

  if (CLUSTER_NAMES.includes(clusterOrUrl as Cluster)) {
    if (clusterOrUrl !== 'mainnet-beta') {
      url.searchParams.append('cluster', clusterOrUrl);
    }
  } else {
    url.searchParams.append('cluster', 'custom');
    url.searchParams.append('customUrl', clusterOrUrl);
  }

  return url;
}
