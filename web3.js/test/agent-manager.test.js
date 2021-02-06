// @flow

import {AgentManager, DESTROY_TIMEOUT_MS} from '../src/agent-manager';
import {expect} from 'chai';
import {sleep} from '../src/util/sleep';

describe('AgentManager', () => {
  it('works', async () => {
    const manager = new AgentManager();
    const agent = manager._agent;
    expect(manager._activeRequests).to.eq(0);
    expect(manager._destroyTimeout).to.be.null;

    manager.requestStart();

    expect(manager._activeRequests).to.eq(1);
    expect(manager._destroyTimeout).to.be.null;

    manager.requestEnd();

    expect(manager._activeRequests).to.eq(0);
    expect(manager._destroyTimeout).not.to.be.null;

    manager.requestStart();
    manager.requestStart();

    expect(manager._activeRequests).to.eq(2);
    expect(manager._destroyTimeout).to.be.null;

    manager.requestEnd();
    manager.requestEnd();

    expect(manager._activeRequests).to.eq(0);
    expect(manager._destroyTimeout).not.to.be.null;
    expect(manager._agent).to.eq(agent);

    await sleep(DESTROY_TIMEOUT_MS);

    expect(manager._agent).not.to.eq(agent);
  }).timeout(2 * DESTROY_TIMEOUT_MS);
});
