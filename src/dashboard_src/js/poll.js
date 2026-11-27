import { el, activeView, pollSpanFor } from './dom.js';
import { pollBackoff } from './state.js';
import { fetchData } from './api.js';
import { t } from './i18n/index.js';

let pollTimer = null;
const POLL_VIEWS = new Set(['dashboard', 'actions']);

function scheduleNextPoll() {
  if (pollTimer) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
  if (document.hidden) return;
  const view = activeView();
  if (!POLL_VIEWS.has(view)) return;

  pollTimer = setTimeout(async () => {
    try {
      await fetchData();
    } finally {
      scheduleNextPoll();
    }
  }, pollBackoff);
}

export function startPoll() {
  stopPoll();
  const spanId = pollSpanFor(activeView());
  if (spanId) {
    const node = el(spanId);
    if (node) node.textContent = t('poll.polling', { sec: (pollBackoff / 1000).toFixed(0) });
  }
  scheduleNextPoll();
}

export function stopPoll() {
  if (pollTimer) {
    clearTimeout(pollTimer);
    pollTimer = null;
  }
  const aEl = el('actions-poll');
  if (aEl) aEl.textContent = '';
  const rEl = el('recent-calls-poll');
  if (rEl) rEl.textContent = '';
  const sEl = el('skills-poll');
  if (sEl) sEl.textContent = '';
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    stopPoll();
  } else {
    const view = activeView();
    if (POLL_VIEWS.has(view)) {
      fetchData().finally(() => startPoll());
    }
  }
});
