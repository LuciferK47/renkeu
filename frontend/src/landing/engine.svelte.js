// Is there a Renkeu server behind this page? A public copy of the site has
// none, and should offer "Get Renkeu"; a copy served by the app does, and
// should offer "Open editor". Asked through the one API client, so the desktop
// shell's injected base URL is honoured.

import { api as defaultApi } from '../lib/api/client.js';

export class EngineStatus {
  /** 'checking' | 'online' | 'offline' */
  state = $state('checking');
  latency = $state(null);
  #api;
  #timer = null;

  constructor({ api = defaultApi } = {}) {
    this.#api = api;
  }

  async check() {
    const started = performance.now();
    try {
      await this.#api.getConfig();
      this.latency = Math.max(1, Math.round(performance.now() - started));
      this.state = 'online';
    } catch {
      this.latency = null;
      this.state = 'offline';
    }
  }

  start(everyMs = 15000) {
    this.check();
    this.#timer = setInterval(() => this.check(), everyMs);
    return () => this.stop();
  }

  stop() {
    clearInterval(this.#timer);
    this.#timer = null;
  }
}
