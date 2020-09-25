// @flow

import {Agent} from 'http';

export const DESTROY_TIMEOUT_MS = 5000;

export class AgentManager {
  _agent: Agent = AgentManager._newAgent();
  _activeRequests = 0;
  _destroyTimeout: TimeoutID | null = null;

  static _newAgent(): Agent {
    return new Agent({keepAlive: true, maxSockets: 25});
  }

  requestStart(): Agent {
    // $FlowExpectedError - Don't manage agents in the browser
    if (process.browser) return;

    this._activeRequests++;
    clearTimeout(this._destroyTimeout);
    this._destroyTimeout = null;
    return this._agent;
  }

  requestEnd() {
    // $FlowExpectedError - Don't manage agents in the browser
    if (process.browser) return;

    this._activeRequests--;
    if (this._activeRequests === 0 && this._destroyTimeout === null) {
      this._destroyTimeout = setTimeout(() => {
        this._agent.destroy();
        this._agent = AgentManager._newAgent();
      }, DESTROY_TIMEOUT_MS);
    }
  }
}
