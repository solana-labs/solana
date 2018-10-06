// @flow

import * as BufferLayout from 'buffer-layout';


/**
 * Layout for a public key
 */
export const publicKey = (property: string = 'publicKey'): Object => {
  return BufferLayout.blob(32, property);
};

/**
 * Layout for a 256bit unsigned value
 */
export const uint256 = (property: string = 'uint256'): Object => {
  return BufferLayout.blob(32, property);
};
