// @flow

import {AgentManager, DESTROY_TIMEOUT_MS} from '../src/agent-manager';
import {sleep} from '../src/util/sleep';

jest.setTimeout(10 * 1000);

test('agent manager', async () => {
  const manager = new AgentManager();
  const agent = manager._agent;
  expect(manager._activeRequests).toBe(0);
  expect(manager._destroyTimeout).toBeNull();

  manager.requestStart();

  expect(manager._activeRequests).toBe(1);
  expect(manager._destroyTimeout).toBeNull();

  manager.requestEnd();

  expect(manager._activeRequests).toBe(0);
  expect(manager._destroyTimeout).not.toBeNull();

  manager.requestStart();
  manager.requestStart();

  expect(manager._activeRequests).toBe(2);
  expect(manager._destroyTimeout).toBeNull();

  manager.requestEnd();
  manager.requestEnd();

  expect(manager._activeRequests).toBe(0);
  expect(manager._destroyTimeout).not.toBeNull();
  expect(manager._agent).toBe(agent);

  await sleep(DESTROY_TIMEOUT_MS);

  expect(manager._agent).not.toBe(agent);
});
