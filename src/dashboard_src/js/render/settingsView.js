import { fetchServerConfig, saveServerConfig, testServerConnection } from '../api.js';
import { showAlert } from '../dialog.js';
import { escapeHtml } from '../dom.js';

let initialized = false;
let clearApiKeyRequested = false;

export async function initSettingsView() {
  if (initialized) return;
  initialized = true;

  const btnTest = document.getElementById('btn-test-connection');
  const btnSave = document.getElementById('btn-save-server-config');
  const btnGenToken = document.getElementById('btn-generate-token');
  const btnClearToken = document.getElementById('btn-clear-token');
  const btnCopy = document.getElementById('btn-copy-cli-command');
  const urlEl = document.getElementById('setting-server-url');
  const keyEl = document.getElementById('setting-server-key');

  if (btnTest) btnTest.addEventListener('click', onTestConnection);
  if (btnSave) btnSave.addEventListener('click', onSaveConfig);
  if (btnGenToken) btnGenToken.addEventListener('click', onGenerateToken);
  if (btnClearToken) btnClearToken.addEventListener('click', onClearToken);
  if (btnCopy) btnCopy.addEventListener('click', onCopyCliCommand);
  if (urlEl) urlEl.addEventListener('input', updateCliHelper);
  if (keyEl) keyEl.addEventListener('input', () => {
    clearApiKeyRequested = false;
  });

  await loadCurrentConfig();
}

export async function loadCurrentConfig() {
  try {
    const res = await fetchServerConfig();
    if (!res || !res.config) return;

    const s = res.config.server || {};
    const d = res.config.dashboard || {};
    const modeEl = document.getElementById('setting-server-mode');
    const urlEl = document.getElementById('setting-server-url');
    const keyEl = document.getElementById('setting-server-key');
    const keyStatusEl = document.getElementById('setting-server-key-status');
    const btnClearToken = document.getElementById('btn-clear-token');
    const timeoutEl = document.getElementById('setting-server-timeout');
    const bindEl = document.getElementById('setting-server-bind');
    const dashPortEl = document.getElementById('setting-dashboard-port');

    const keyConfigured = Boolean(res.api_key_configured);
    clearApiKeyRequested = false;

    if (modeEl) modeEl.value = s.mode || 'local';
    if (urlEl) urlEl.value = s.url || 'http://127.0.0.1:11998';
    if (keyEl) {
      keyEl.value = '';
      keyEl.placeholder = keyConfigured
        ? 'API key configured (leave blank to keep current)'
        : 'Optional token if server binds to 0.0.0.0';
    }
    if (keyStatusEl) {
      keyStatusEl.textContent = keyConfigured
        ? 'An API key is stored on the server. Enter a new token to replace it, or click Clear Key.'
        : 'No API key configured.';
    }
    if (btnClearToken) btnClearToken.classList.toggle('hidden', !keyConfigured);
    if (timeoutEl) timeoutEl.value = s.timeout_ms || 3500;
    if (bindEl) bindEl.value = d.bind || '127.0.0.1';
    if (dashPortEl) dashPortEl.value = d.port || 11997;

    updateCliHelper();
  } catch (e) {
    console.warn('Failed to load server config:', e);
  }
}

function updateCliHelper() {
  const urlEl = document.getElementById('setting-server-url');
  const codeEl = document.getElementById('cli-helper-command');
  const url = (urlEl && urlEl.value.trim()) || 'http://127.0.0.1:11998';
  if (codeEl) {
    codeEl.textContent = `agent-guidance --set-server ${url}`;
  }
}

function onGenerateToken() {
  const keyEl = document.getElementById('setting-server-key');
  if (!keyEl) return;
  const randArr = new Uint8Array(24);
  window.crypto.getRandomValues(randArr);
  const hex = Array.from(randArr).map(b => b.toString(16).padStart(2, '0')).join('');
  keyEl.value = `ag_sec_${hex}`;
  clearApiKeyRequested = false;
}

function onClearToken() {
  const keyEl = document.getElementById('setting-server-key');
  const keyStatusEl = document.getElementById('setting-server-key-status');
  clearApiKeyRequested = true;
  if (keyEl) {
    keyEl.value = '';
    keyEl.placeholder = 'API key will be cleared on save';
  }
  if (keyStatusEl) {
    keyStatusEl.textContent = 'API key marked for removal. Click Apply & Auto-Configure to save.';
  }
}

async function onCopyCliCommand() {
  const codeEl = document.getElementById('cli-helper-command');
  const btn = document.getElementById('btn-copy-cli-command');
  if (!codeEl) return;
  try {
    await navigator.clipboard.writeText(codeEl.textContent);
    if (btn) {
      const orig = btn.textContent;
      btn.textContent = '✓ Copied!';
      setTimeout(() => { btn.textContent = orig; }, 2000);
    }
  } catch (e) {
    console.warn('Clipboard write failed:', e);
  }
}

async function onTestConnection() {
  const urlEl = document.getElementById('setting-server-url');
  const keyEl = document.getElementById('setting-server-key');
  const timeoutEl = document.getElementById('setting-server-timeout');
  const resultEl = document.getElementById('test-connection-result');
  const btn = document.getElementById('btn-test-connection');

  const url = urlEl ? urlEl.value.trim() : '';
  if (!url) {
    if (resultEl) {
      resultEl.innerHTML = '<span class="status-badge status-err">Missing Server URL</span>';
    }
    return;
  }

  const key = keyEl ? keyEl.value.trim() : '';
  const timeout = timeoutEl ? parseInt(timeoutEl.value, 10) || 3500 : 3500;

  if (btn) btn.disabled = true;
  if (resultEl) {
    resultEl.innerHTML = '<span style="color:var(--text-secondary)">Testing connection...</span>';
  }

  try {
    const res = await testServerConnection({
      url,
      api_key: key || undefined,
      timeout_ms: timeout,
    });

    if (res.status === 'ok') {
      const h = res.server_health || {};
      const latency = res.latency_ms !== undefined ? `${res.latency_ms} ms` : 'OK';
      resultEl.innerHTML = `
        <div style="background:rgba(16,185,129,0.1);border:1px solid #10b981;border-radius:6px;padding:8px 12px;margin-top:8px">
          <div style="color:#10b981;font-weight:600;margin-bottom:4px">✓ Connected (${escapeHtml(latency)})</div>
          <div style="font-size:12px;color:var(--text-secondary)">
            Server: ${escapeHtml(h.engine || 'rust-candle')} (${escapeHtml(h.backend || 'candle-bert')}) | Version: ${escapeHtml(h.version || 'unknown')}
          </div>
          <div style="font-size:12px;color:var(--text-secondary)">
            Skills Loaded: ${Number(h.total_skills) || 0} (${Number(h.total_sections) || 0} sections)
          </div>
        </div>
      `;
    } else {
      resultEl.innerHTML = `
        <div style="background:rgba(239,68,68,0.1);border:1px solid #ef4444;border-radius:6px;padding:8px 12px;margin-top:8px">
          <div style="color:#ef4444;font-weight:600">✗ Connection Failed</div>
          <div style="font-size:12px;color:var(--text-secondary)">${escapeHtml(res.message || 'Unknown error')}</div>
        </div>
      `;
    }
  } catch (e) {
    if (resultEl) {
      resultEl.innerHTML = `
        <div style="background:rgba(239,68,68,0.1);border:1px solid #ef4444;border-radius:6px;padding:8px 12px;margin-top:8px">
          <div style="color:#ef4444;font-weight:600">✗ Connection Error</div>
          <div style="font-size:12px;color:var(--text-secondary)">${escapeHtml(e.message)}</div>
        </div>
      `;
    }
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function onSaveConfig() {
  const modeEl = document.getElementById('setting-server-mode');
  const urlEl = document.getElementById('setting-server-url');
  const keyEl = document.getElementById('setting-server-key');
  const timeoutEl = document.getElementById('setting-server-timeout');
  const bindEl = document.getElementById('setting-server-bind');
  const dashPortEl = document.getElementById('setting-dashboard-port');
  const autoIdeEl = document.getElementById('setting-server-auto-ide');
  const btn = document.getElementById('btn-save-server-config');

  const mode = modeEl ? modeEl.value : 'remote';
  const url = urlEl ? urlEl.value.trim() : '';
  const key = keyEl ? keyEl.value.trim() : '';
  const timeout = timeoutEl ? parseInt(timeoutEl.value, 10) || 3500 : 3500;
  const dashboardBind = bindEl ? bindEl.value : '127.0.0.1';
  const dashboardPort = dashPortEl ? parseInt(dashPortEl.value, 10) || 11997 : 11997;
  const autoIde = autoIdeEl ? autoIdeEl.checked : true;

  if (btn) btn.disabled = true;

  try {
    const res = await saveServerConfig({
      mode,
      url,
      api_key: key || undefined,
      clear_api_key: clearApiKeyRequested,
      timeout_ms: timeout,
      dashboard_bind: dashboardBind,
      dashboard_port: dashboardPort,
      auto_configure_ide: autoIde,
    });

    await loadCurrentConfig();
    await showAlert({
      title: 'Configuration Saved',
      message: `${res.message}\nIDE Status: ${res.ide_configuration || 'Updated'}`,
      variant: 'success',
    });
  } catch (e) {
    await showAlert({
      title: 'Save Failed',
      message: e.message,
      variant: 'danger',
    });
  } finally {
    if (btn) btn.disabled = false;
  }
}
