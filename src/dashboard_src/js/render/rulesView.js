import { el } from '../dom.js';
import { fetchToolRules, saveToolRules, revertToolRules } from '../api.js';
import { t } from '../i18n/index.js';

let currentRulesData = null;

export async function renderRulesView() {
  const container = el('view-rules');
  if (!container) return;

  const projectSelect = el('project-selector');
  const project = projectSelect ? projectSelect.value : 'all';

  container.innerHTML = `<div class="loading" style="padding:24px;text-align:center">${t('common.loading') || 'Loading...'}</div>`;

  try {
    currentRulesData = await fetchToolRules(project);
    buildRulesHtml(container, currentRulesData, project);
  } catch (err) {
    container.innerHTML = `<div class="error-banner" style="display:block;margin:16px;">Failed to load rules: ${err.message}</div>`;
  }
}

function buildRulesHtml(container, data, project) {
  const isGlobal = !project || project === 'all';
  const hasOverride = data.has_project_override;

  let scopeBadge = isGlobal
    ? `<span class="badge badge-global">🌐 Global Config</span>`
    : hasOverride
    ? `<span class="badge badge-override">📁 Project Override</span>`
    : `<span class="badge badge-inherited">🔗 Inherited from Global</span>`;

  let revertBtnHtml = (!isGlobal && hasOverride)
    ? `<button id="btn-revert-rules" class="btn btn-sm btn-danger" style="margin-right:8px;">↩️ ${t('rules.revert_global') || 'Revert to Global'}</button>`
    : '';

  let html = `
    <div class="rules-view-header" style="display:flex;justify-content:space-between;align-items:center;padding:16px 20px;border-bottom:1px solid var(--border-color);background:var(--bg-secondary);flex-wrap:wrap;gap:12px;">
      <div>
        <h2 style="margin:0 0 4px 0;font-size:18px;display:flex;align-items:center;gap:8px;">
          <span>⚡ ${t('rules.title') || 'Tool Micro-Rules'}</span>
          ${scopeBadge}
        </h2>
        <div style="font-size:12px;color:var(--text-secondary);">
          ${isGlobal ? (t('rules.global_desc') || 'Default rules applied to all workspaces.') : (t('rules.project_desc') || 'Custom rules for this project:') + ' <code>' + project + '</code>'}
        </div>
      </div>
      <div style="display:flex;align-items:center;gap:8px;">
        ${revertBtnHtml}
        <button id="btn-save-rules" class="btn btn-sm btn-primary">💾 ${t('rules.save_all') || 'Save Changes'}</button>
      </div>
    </div>
    <div id="rules-status-msg" style="display:none;padding:8px 20px;font-size:13px;"></div>
    <div class="rules-grid" style="display:grid;grid-template-columns:repeat(auto-fill, minmax(360px, 1fr));gap:16px;padding:20px;">
  `;

  for (const tool of data.tools) {
    const scopeClass = tool.scope === 'project' ? 'badge-override' : (tool.scope === 'global' ? 'badge-global' : 'badge-default');
    const scopeLabel = tool.scope === 'project' ? 'Project' : (tool.scope === 'global' ? 'Global' : 'Default');

    html += `
      <div class="card rule-card" data-tool="${tool.name}" style="background:var(--bg-secondary);border:1px solid var(--border-color);border-radius:var(--border-radius-md);padding:14px;display:flex;flex-direction:column;gap:10px;">
        <div style="display:flex;justify-content:space-between;align-items:center;">
          <div style="display:flex;align-items:center;gap:8px;">
            <strong style="font-family:var(--font-mono);font-size:14px;color:var(--color-primary);">${tool.name}</strong>
            <span class="badge ${scopeClass}" style="font-size:10px;">${scopeLabel}</span>
          </div>
          <label class="switch" title="Enable/Disable rule" style="display:flex;align-items:center;gap:6px;cursor:pointer;font-size:12px;color:var(--text-secondary);">
            <input type="checkbox" class="rule-enable-toggle" data-tool="${tool.name}" ${tool.enabled ? 'checked' : ''}>
            <span>${tool.enabled ? 'Enabled' : 'Disabled'}</span>
          </label>
        </div>
        <div>
          <textarea class="rule-text-input" data-tool="${tool.name}" rows="4" style="width:100%;box-sizing:border-box;resize:vertical;font-family:var(--font-mono);font-size:12px;padding:8px;border-radius:var(--border-radius-sm);background:var(--bg-tertiary);color:var(--text-primary);border:1px solid var(--border-color);line-height:1.4;">${escapeHtml(tool.rule)}</textarea>
        </div>
        <div style="display:flex;justify-content:space-between;align-items:center;margin-top:auto;pt:4px;">
          <button class="btn btn-xs btn-outline btn-reset-default" data-tool="${tool.name}" data-default="${escapeAttr(tool.default_rule)}" style="font-size:11px;padding:3px 8px;">🔄 ${t('rules.reset_default') || 'Reset Default'}</button>
          <span style="font-size:11px;color:var(--text-muted);">${tool.name === 'task_pipeline' ? 'Turn 1 Init' : (tool.name === 'project_context' ? 'Code Reader' : 'MCP Guard')}</span>
        </div>
      </div>
    `;
  }

  html += `</div>`;
  container.innerHTML = html;
  bindRulesEvents(container, project);
}

function bindRulesEvents(container, project) {
  const saveBtn = container.querySelector('#btn-save-rules');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      saveBtn.disabled = true;
      saveBtn.textContent = 'Saving...';
      const rules = {};
      container.querySelectorAll('.rule-card').forEach(card => {
        const name = card.dataset.tool;
        const enabled = card.querySelector('.rule-enable-toggle')?.checked ?? true;
        const rule = card.querySelector('.rule-text-input')?.value ?? '';
        rules[name] = { enabled, rule };
      });

      try {
        await saveToolRules(project, rules);
        showStatusMsg('Rules saved successfully!', 'success');
        await renderRulesView();
      } catch (e) {
        showStatusMsg('Failed to save rules: ' + e.message, 'error');
        saveBtn.disabled = false;
        saveBtn.textContent = '💾 ' + (t('rules.save_all') || 'Save Changes');
      }
    });
  }

  const revertBtn = container.querySelector('#btn-revert-rules');
  if (revertBtn) {
    revertBtn.addEventListener('click', async () => {
      if (!confirm('Revert project rules to global configuration?')) return;
      revertBtn.disabled = true;
      try {
        await revertToolRules(project);
        showStatusMsg('Project rules reverted to global!', 'success');
        await renderRulesView();
      } catch (e) {
        showStatusMsg('Failed to revert: ' + e.message, 'error');
        revertBtn.disabled = false;
      }
    });
  }

  container.querySelectorAll('.btn-reset-default').forEach(btn => {
    btn.addEventListener('click', () => {
      const toolName = btn.dataset.tool;
      const defaultRule = btn.dataset.default;
      const card = container.querySelector(`.rule-card[data-tool="${toolName}"]`);
      if (card) {
        const textarea = card.querySelector('.rule-text-input');
        const toggle = card.querySelector('.rule-enable-toggle');
        if (textarea) textarea.value = defaultRule;
        if (toggle) {
          toggle.checked = true;
          const label = toggle.nextElementSibling;
          if (label) label.textContent = 'Enabled';
        }
      }
    });
  });

  container.querySelectorAll('.rule-enable-toggle').forEach(t => {
    t.addEventListener('change', () => {
      const label = t.nextElementSibling;
      if (label) label.textContent = t.checked ? 'Enabled' : 'Disabled';
    });
  });
}

function showStatusMsg(msg, type) {
  const elMsg = el('rules-status-msg');
  if (!elMsg) return;
  elMsg.style.display = 'block';
  elMsg.style.color = type === 'success' ? 'var(--color-success, #22c55e)' : 'var(--color-danger, #ef4444)';
  elMsg.style.background = type === 'success' ? 'rgba(34, 197, 94, 0.1)' : 'rgba(239, 68, 68, 0.1)';
  elMsg.textContent = msg;
  setTimeout(() => { if (elMsg) elMsg.style.display = 'none'; }, 4000);
}

function escapeHtml(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function escapeAttr(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/"/g, '&quot;');
}
