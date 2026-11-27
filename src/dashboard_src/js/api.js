import { setText, setDisplay, setLoading, activeView, pollSpanFor } from './dom.js';
import { renderDashboard, getActiveTimeframe } from './render/statsView.js';
import { renderHealthPanel } from './render/health.js';
import { pollBackoff, setBackoff, resetBackoff, store } from './state.js';
import { showAlert } from './dialog.js';
import { t } from './i18n/index.js';

let selectedProject = 'all';

function isLoopbackHost() {
  if (typeof window === 'undefined' || !window.location) return true;
  const host = (window.location.hostname || '').toLowerCase();
  return host === '127.0.0.1' || host === 'localhost' || host === '::1' || host === '[::1]';
}

export async function apiFetch(url, options = {}) {
  const headers = new Headers(options.headers || {});
  const token = typeof sessionStorage !== 'undefined' ? sessionStorage.getItem('ag_api_token') : null;
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  let resp = await fetch(url, { ...options, headers });
  if (resp.status === 401 && !isLoopbackHost() && typeof window !== 'undefined' && typeof window.prompt === 'function') {
    const entered = window.prompt('Enter API Bearer token for remote dashboard access:');
    if (entered && entered.trim()) {
      const clean = entered.trim();
      if (typeof sessionStorage !== 'undefined') sessionStorage.setItem('ag_api_token', clean);
      headers.set('Authorization', `Bearer ${clean}`);
      resp = await fetch(url, { ...options, headers });
    }
  }
  return resp;
}

export function setSelectedProject(proj) {
  selectedProject = proj;
}

export function getSelectedProject() {
  return selectedProject;
}

async function fetchStats() {
  const projParam = selectedProject ? `&project=${encodeURIComponent(selectedProject)}` : '&project=all';
  const tf = getActiveTimeframe ? getActiveTimeframe() : 'past_24h';
  const tfParam = tf ? `&timeframe=${encodeURIComponent(tf)}` : '';
  const resp = await apiFetch(`/api/stats?window=24h${projParam}${tfParam}`);
  if (!resp.ok) throw new Error('HTTP ' + resp.status);
  return resp.json();
}

export async function fetchGraphData(optionsOrProj) {
  let p = selectedProject;
  let view = 'symbols';
  let file = null;

  if (typeof optionsOrProj === 'string') {
    p = optionsOrProj;
  } else if (optionsOrProj && typeof optionsOrProj === 'object') {
    if (optionsOrProj.proj !== undefined) p = optionsOrProj.proj;
    else if (optionsOrProj.project !== undefined) p = optionsOrProj.project;
    if (optionsOrProj.view) view = optionsOrProj.view;
    if (optionsOrProj.file) file = optionsOrProj.file;
  }

  const params = new URLSearchParams();
  if (p && p !== 'all') params.set('project', p);
  if (view && view !== 'symbols') params.set('view', view);
  if (file) params.set('file', file);

  const qs = params.toString() ? `?${params.toString()}` : '';
  try {
    const resp = await apiFetch(`/api/graph${qs}`);
    if (!resp.ok) return { graph_available: false, message: `HTTP ${resp.status}` };
    return resp.json();
  } catch (e) {
    return { graph_available: false, message: e.message };
  }
}

export async function fetchProjects() {
  try {
    const resp = await apiFetch('/api/projects');
    if (!resp.ok) return [];
    const data = await resp.json();
    return data.projects || [];
  } catch (e) {
    return [];
  }
}

export async function pruneProjects() {
  try {
    const resp = await apiFetch('/api/projects/prune', { method: 'POST' });
    return resp.json();
  } catch (e) {
    return { success: false, error: e.message };
  }
}

export async function deleteProject(projectPath, deleteIndex = true) {
  try {
    const resp = await apiFetch('/api/projects/delete', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ project: projectPath, delete_index: deleteIndex }),
    });
    return resp.json();
  } catch (e) {
    return { success: false, error: e.message };
  }
}

export async function triggerAutoCleanup() {
  try {
    const resp = await apiFetch('/api/cleanup', { method: 'POST' });
    return resp.json();
  } catch (e) {
    return { success: false, error: e.message };
  }
}

export async function cleanDashboardMetrics(project = 'all') {
  try {
    const resp = await apiFetch('/api/dashboard/clean', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ project }),
    });
    return resp.json();
  } catch (e) {
    return { success: false, error: e.message };
  }
}

async function fetchHealth() {
  const resp = await apiFetch('/health');
  return resp.json();
}

async function showFetchError(e) {
  setBackoff(Math.min(pollBackoff * 2, 30000));
  setText('error-banner', t('api.connect_failed', { error: e.message || t('status.unknown') }));
  setDisplay('error-banner', 'block');
  const pollSpanId = pollSpanFor(activeView());
  if (pollSpanId) {
    setText(pollSpanId, t('api.retry_in', { sec: (pollBackoff / 1000).toFixed(0) }));
  }
}

async function clearFetchError() {
  resetBackoff();
  setText('error-banner', '');
  setDisplay('error-banner', 'none');
}

function computeDataHash(data) {
  if (!data) return '';
  const totals = data.totals || {};
  const recent = (data.recent_actions && data.recent_actions[0]?.id) || '';
  const calls = totals.tool_calls || 0;
  const opt = totals.tokens_optimized || 0;
  const skillsLen = (data.top_skills || []).length;
  const actionsLen = (data.tool_breakdown || []).length;
  return `${calls}:${opt}:${skillsLen}:${actionsLen}:${recent}:${data.server_port || ''}:${data.project_path || ''}`;
}

export async function fetchData(options = {}) {
  const { force = false } = options;
  if (store.is_fetching) return;
  store.is_fetching = true;
  setLoading(true);

  try {
    const [statsResult, hdata] = await Promise.all([
      fetchStats().then(d => ({ data: d, error: null })).catch(e => ({ data: null, error: e })),
      fetchHealth().catch(() => ({ status: 'unknown' })),
    ]);

    if (statsResult.error) {
      console.error('fetch error', statsResult.error);
      await showFetchError(statsResult.error);
    } else if (statsResult.data) {
      const newHash = computeDataHash(statsResult.data);
      if (force || newHash !== store.last_stats_hash) {
        store.last_stats_hash = newHash;
        renderDashboard(statsResult.data);
      }
      await clearFetchError();
    }

    renderHealthPanel(hdata);
    if (hdata && hdata.db_size_bytes !== undefined) {
      const mb = (hdata.db_size_bytes / (1024 * 1024)).toFixed(2);
      setText('sidebar-db-size', t('sidebar.db_size_val', { size: mb }));
    }
  } catch (err) {
    console.error('fetchData unexpected error', err);
  } finally {
    store.is_fetching = false;
    setLoading(false);
  }
}

export async function refreshEmbedEngine() {
  try {
    const resp = await apiFetch('/api/engine/refresh', { method: 'POST' });
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    return await resp.json();
  } catch (e) {
    return { success: false, error: e.message };
  }
}

export async function refreshEmbedStatus() {
  const btn = document.getElementById('btn-embed-refresh');
  const txt = document.getElementById('btn-embed-refresh-text');
  if (btn) btn.disabled = true;
  if (txt) {
    txt.textContent = t('deck.refreshing');
  } else if (btn) {
    btn.textContent = t('deck.refreshing');
  }
  try {
    const res = await refreshEmbedEngine();
    const [stats, health] = await Promise.all([fetchStats().catch(() => null), fetchHealth().catch(() => null)]);
    if (stats) renderDashboard(stats);
    if (health) renderHealthPanel(health);
    await showAlert({
      title: t('deck.engine_control'),
      message: res.message || (res.success ? 'Engine refreshed' : (res.error || 'Refresh failed')),
      variant: res.success ? 'success' : 'danger',
    });
  } finally {
    if (btn) btn.disabled = false;
    if (txt) {
      txt.textContent = t('deck.refresh_model');
    } else if (btn) {
      btn.textContent = t('deck.refresh_model');
    }
  }
}

export async function fetchLogs({ level = 'all', search = '', limit = 50, offset = 0 } = {}) {
  const query = new URLSearchParams();
  if (level && level !== 'all') query.set('level', level);
  if (search && search.trim()) query.set('search', search.trim());
  if (limit) query.set('limit', limit);
  if (offset) query.set('offset', offset);

  const res = await apiFetch('/api/logs?' + query.toString());
  if (!res.ok) throw new Error('Failed to fetch logs: ' + res.statusText);
  return res.json();
}

export async function clearLogs(level = 'all') {
  const query = level && level !== 'all' ? `?level=${encodeURIComponent(level)}` : '';
  const res = await apiFetch('/api/logs/clear' + query, { method: 'POST' });
  if (!res.ok) throw new Error('Failed to clear logs: ' + res.statusText);
  return res.json();
}

export async function fetchServerConfig() {
  const resp = await apiFetch('/api/config/server');
  if (!resp.ok) throw new Error('Failed to fetch server config: ' + resp.statusText);
  return resp.json();
}

export async function saveServerConfig(payload) {
  const resp = await apiFetch('/api/config/server', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!resp.ok) throw new Error('Failed to save server config: ' + resp.statusText);
  return resp.json();
}

export async function testServerConnection(payload) {
  const resp = await apiFetch('/api/config/test', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!resp.ok) throw new Error('Failed to test connection: ' + resp.statusText);
  return resp.json();
}

export async function fetchSkillsRegistry() {
  const resp = await apiFetch('/api/skills/registry');
  if (!resp.ok) throw new Error('Failed to fetch skill registry: ' + resp.statusText);
  return resp.json();
}

export async function fetchSkillsBinaryStats() {
  const resp = await apiFetch('/api/skills/binary_stats');
  if (!resp.ok) throw new Error('Failed to fetch binary stats: ' + resp.statusText);
  return resp.json();
}

export async function deleteBulkSkills(names, purgeStaging = true) {
  const resp = await apiFetch('/api/skills/delete_bulk', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ names, purge_staging: purgeStaging }),
  });
  if (!resp.ok) throw new Error('Failed to delete skills: ' + resp.statusText);
  return resp.json();
}

export async function compileSkillsBinary(force = false) {
  const resp = await apiFetch('/api/skills/compile', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ force }),
  });
  if (!resp.ok) throw new Error('Failed to compile skills: ' + resp.statusText);
  return resp.json();
}

export { fetchToolRules, saveToolRules, revertToolRules } from './rulesApi.js';


