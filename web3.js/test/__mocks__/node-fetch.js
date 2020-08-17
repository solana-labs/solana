// @flow

import fetch from 'node-fetch';

type RpcRequest = {
  method: string,
  params?: Array<any>,
};

type RpcResponseError = {
  message: string,
};
type RpcResponseResult = any;
type RpcResponse = {
  error: ?RpcResponseError,
  result: ?RpcResponseResult,
};

export const mockRpc: Array<[string, RpcRequest, RpcResponse]> = [];

// Define TEST_LIVE in the environment to test against the real full node
// identified by `url` instead of using the mock
export const mockRpcEnabled = !process.env.TEST_LIVE;

let mockNotice = true;

// Suppress lint: 'JestMockFn' is not defined
// eslint-disable-next-line no-undef
const mock: JestMockFn<any, any> = jest.fn((fetchUrl, fetchOptions) => {
  if (!mockRpcEnabled) {
    if (mockNotice) {
      console.log(
        `Note: node-fetch mock is disabled, testing live against ${fetchUrl}`,
      );
      mockNotice = false;
    }
    return fetch(fetchUrl, fetchOptions);
  }

  expect(mockRpc.length).toBeGreaterThanOrEqual(1);
  const [mockUrl, mockRequest, mockResponse] = mockRpc.shift();

  expect(fetchUrl).toBe(mockUrl);
  expect(fetchOptions).toMatchObject({
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
  });
  expect(fetchOptions.body).toBeDefined();

  const body = JSON.parse(fetchOptions.body);
  expect(body).toMatchObject(
    Object.assign(
      {},
      {
        jsonrpc: '2.0',
        method: 'invalid',
      },
      mockRequest,
    ),
  );

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
    ok: true,
    status: 200,
    statusText: 'OK',
    text: () => {
      return Promise.resolve(JSON.stringify(response));
    },
  };
});

export default mock;
