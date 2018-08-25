// @flow

import fetch from 'node-fetch';

type RpcRequest = {
  method: string;
  params?: Array<any>;
};

type RpcResponseError = {
  message: string;
}
type RpcResponseResult = boolean | string | number;
type RpcResponse = {
  error: ?RpcResponseError;
  result: ?RpcResponseResult;
};

export const mockRpc: Array<[string, RpcRequest, RpcResponse]> = [];

// Suppress lint: 'JestMockFn' is not defined
// eslint-disable-next-line no-undef
const mock: JestMockFn<any, any> = jest.fn(
  (fetchUrl, fetchOptions) => {
    // Define DOITLIVE in the environment to test against the real full node
    // identified by `url` instead of using the mock
    if (process.env.DOITLIVE) {
      console.log(`Note: node-fetch mock is disabled, testing live against ${fetchUrl}`);
      return fetch(fetchUrl, fetchOptions);
    }

    expect(mockRpc.length).toBeGreaterThanOrEqual(1);
    const [mockUrl, mockRequest, mockResponse] = mockRpc.shift();

    expect(fetchUrl).toBe(mockUrl);
    expect(fetchOptions).toMatchObject({
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
    });
    expect(fetchOptions.body).toBeDefined();

    const body = JSON.parse(fetchOptions.body);
    expect(body).toMatchObject(Object.assign(
      {},
      {
        jsonrpc: '2.0',
        method: 'invalid',
      },
      mockRequest
    ));

    const response = Object.assign(
      {},
      {
        jsonrpc: '2.0',
        id: body.id,
        error: {
          message: 'invalid error message',
        },
        result: 'invalid response',
      },
      mockResponse,
    );
    return {
      text: () => {
        return Promise.resolve(JSON.stringify(response));
      },
    };
  }
);

export default mock;
