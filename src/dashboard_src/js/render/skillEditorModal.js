import { fetchSkillContent, saveSkillContent, revertSkillContent } from '../skillsApi.js';
import { showAlert, showConfirm } from '../dialog.js';

let activeCallback = null;

export async function openSkillEditorModal(skillName = '', onSaved = null) {
  activeCallback = onSaved;
  const isNew = !skillName;
  let modal = document.getElementById('skill-editor-modal');

  if (!modal) {
    modal = document.createElement('div');
    modal.id = 'skill-editor-modal';
    modal.className = 'modal-backdrop';
    document.body.appendChild(modal);
  }

  modal.innerHTML = `
    <div class="modal-dialog" style="max-width:940px;width:95vw;height:85vh;display:flex;flex-direction:column;padding:0">
      <div style="padding:14px 20px;border-bottom:1px solid var(--border-color);display:flex;justify-content:space-between;align-items:center;background:var(--bg-tertiary)">
        <div style="display:flex;align-items:center;gap:12px">
          <span style="font-weight:700;font-size:15px">${isNew ? 'Create New Skill' : 'Edit Skill'}</span>
          <span id="editor-source-badge" style="font-size:11px;padding:2px 8px;border-radius:4px;background:var(--bg-primary);color:var(--text-secondary)">
            ${isNew ? 'New Custom' : 'Loading...'}
          </span>
        </div>
        <button id="btn-close-editor-modal" class="btn-secondary" style="padding:4px 8px;font-size:14px">&times;</button>
      </div>

      <div style="padding:12px 20px;border-bottom:1px solid var(--border-color);background:var(--bg-secondary);display:flex;gap:10px;align-items:center">
        <label style="font-size:12px;font-weight:600;color:var(--text-secondary);min-width:70px">Skill Slug:</label>
        <input type="text" id="editor-skill-name" value="${skillName}" placeholder="e.g. my-custom-skill"
          ${isNew ? '' : 'readonly'} style="flex:1;padding:6px 10px;font-family:var(--font-mono);font-size:13px;border-radius:4px;border:1px solid var(--border-color);background:${isNew ? 'var(--bg-primary)' : 'var(--bg-tertiary)'};color:var(--text-primary)">
      </div>

      <div style="flex:1;display:grid;grid-template-columns:1fr 1fr;overflow:hidden;background:var(--bg-primary)">
        <div style="display:flex;flex-direction:column;border-right:1px solid var(--border-color);height:100%">
          <div style="padding:6px 14px;font-size:11px;font-weight:600;text-transform:uppercase;color:var(--text-secondary);background:var(--bg-secondary);border-bottom:1px solid var(--border-color)">
            Markdown Editor
          </div>
          <textarea id="editor-skill-content" spellcheck="false" placeholder="Enter skill markdown..."
            style="flex:1;width:100%;resize:none;border:none;padding:12px 14px;background:transparent;color:var(--text-primary);font-family:var(--font-mono);font-size:12.5px;line-height:1.5;outline:none"></textarea>
        </div>

        <div style="display:flex;flex-direction:column;height:100%">
          <div style="padding:6px 14px;font-size:11px;font-weight:600;text-transform:uppercase;color:var(--text-secondary);background:var(--bg-secondary);border-bottom:1px solid var(--border-color)">
            Live Preview
          </div>
          <div id="editor-skill-preview" style="flex:1;padding:14px 18px;overflow-y:auto;font-size:13px;line-height:1.6;color:var(--text-primary)">
          </div>
        </div>
      </div>

      <div style="padding:12px 20px;border-top:1px solid var(--border-color);background:var(--bg-tertiary);display:flex;justify-content:space-between;align-items:center">
        <div>
          <button id="btn-editor-revert" class="btn-secondary" style="display:none;color:#ef4444;border-color:rgba(239,68,68,0.3)">
            ↺ Revert to Default
          </button>
        </div>
        <div style="display:flex;gap:10px">
          <button id="btn-editor-cancel" class="btn-secondary">Cancel</button>
          <button id="btn-editor-save" class="btn-primary" style="padding:7px 18px">Save Skill</button>
        </div>
      </div>
    </div>
  `;

  modal.style.display = 'flex';
  bindModalEvents(modal, isNew);

  if (isNew) {
    const starter = `---\nname: my-new-skill\ndescription: Describe what this skill does\n---\n\n# My New Skill\n\n## Guidelines\n- Rule 1\n- Rule 2\n`;
    document.getElementById('editor-skill-content').value = starter;
    updatePreview(starter);
  } else {
    await loadExistingSkill(skillName);
  }
}

async function loadExistingSkill(name) {
  const badge = document.getElementById('editor-source-badge');
  const textarea = document.getElementById('editor-skill-content');
  const revertBtn = document.getElementById('btn-editor-revert');

  try {
    const data = await fetchSkillContent(name);
    textarea.value = data.content || '';
    updatePreview(data.content || '');

    if (data.source === 'staging') {
      badge.textContent = 'Staged Override';
      badge.style.background = 'rgba(168,85,247,0.15)';
      badge.style.color = '#a855f7';
      if (revertBtn) revertBtn.style.display = 'inline-block';
    } else {
      badge.textContent = 'Embedded Default';
      badge.style.background = 'rgba(59,130,246,0.15)';
      badge.style.color = '#3b82f6';
      if (revertBtn) revertBtn.style.display = 'none';
    }
  } catch (err) {
    showAlert({ title: 'Error Loading Skill', message: err.message, variant: 'danger' });
  }
}

function bindModalEvents(modal, isNew) {
  const closeBtn = document.getElementById('btn-close-editor-modal');
  const cancelBtn = document.getElementById('btn-editor-cancel');
  const saveBtn = document.getElementById('btn-editor-save');
  const revertBtn = document.getElementById('btn-editor-revert');
  const textarea = document.getElementById('editor-skill-content');

  const onKey = (e) => {
    if (e.key === 'Escape') closeModal();
  };
  const closeModal = () => {
    modal.style.display = 'none';
    window.removeEventListener('keydown', onKey);
  };

  modal.onclick = (e) => {
    if (e.target === modal) closeModal();
  };
  window.addEventListener('keydown', onKey);

  if (closeBtn) closeBtn.onclick = closeModal;
  if (cancelBtn) cancelBtn.onclick = closeModal;

  if (textarea) {
    textarea.addEventListener('input', () => updatePreview(textarea.value));
    textarea.addEventListener('keydown', (e) => {
      if (e.key === 'Tab') {
        e.preventDefault();
        const start = textarea.selectionStart;
        textarea.value = textarea.value.substring(0, start) + '  ' + textarea.value.substring(textarea.selectionEnd);
        textarea.selectionStart = textarea.selectionEnd = start + 2;
        updatePreview(textarea.value);
      }
    });
  }

  if (saveBtn) saveBtn.onclick = async () => onSaveClick(modal, isNew);
  if (revertBtn) revertBtn.onclick = async () => onRevertClick(modal);
}

async function onSaveClick(modal, isNew) {
  const nameInput = document.getElementById('editor-skill-name');
  const textarea = document.getElementById('editor-skill-content');
  const name = nameInput.value.trim().toLowerCase();
  const content = textarea.value;

  if (!name || !/^[a-z0-9_-]+$/.test(name)) {
    showAlert({ title: 'Invalid Name', message: 'Skill slug must only contain lowercase letters, numbers, hyphens or underscores.', variant: 'danger' });
    return;
  }
  if (!content.trim()) {
    showAlert({ title: 'Empty Content', message: 'Skill markdown content cannot be empty.', variant: 'danger' });
    return;
  }

  const saveBtn = document.getElementById('btn-editor-save');
  saveBtn.disabled = true;
  try {
    await saveSkillContent(name, content);
    modal.style.display = 'none';
    showAlert({ title: 'Skill Saved', message: `Skill '${name}' saved to global staging override. Use 'Compile Staging to Binary' when ready to re-index.`, variant: 'success' });
    if (activeCallback) activeCallback(name);
  } catch (err) {
    showAlert({ title: 'Save Failed', message: err.message, variant: 'danger' });
  } finally {
    saveBtn.disabled = false;
  }
}

async function onRevertClick(modal) {
  const nameInput = document.getElementById('editor-skill-name');
  const name = nameInput.value.trim();

  const confirm = await showConfirm({
    title: `Revert '${name}' to Default?`,
    message: `This will remove the staging override and restore the original embedded version of '${name}'.`,
    confirmText: 'Revert Override',
    variant: 'danger',
  });
  if (!confirm) return;

  try {
    await revertSkillContent(name);
    modal.style.display = 'none';
    showAlert({ title: 'Skill Reverted', message: `'${name}' has been reverted to embedded default.`, variant: 'success' });
    if (activeCallback) activeCallback(name);
  } catch (err) {
    showAlert({ title: 'Revert Failed', message: err.message, variant: 'danger' });
  }
}

function updatePreview(raw) {
  const container = document.getElementById('editor-skill-preview');
  if (!container) return;
  if (!raw || !raw.trim()) {
    container.innerHTML = '<span style="color:var(--text-secondary);font-style:italic">Preview will appear here...</span>';
    return;
  }

  let text = raw.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  text = text.replace(/```([a-z0-9_-]*)\n([\s\S]*?)```/g, (_, lang, code) =>
    `<pre style="background:var(--bg-tertiary);border:1px solid var(--border-color);padding:8px 12px;border-radius:4px;overflow-x:auto;margin:8px 0;font-size:12px"><code>${code}</code></pre>`
  );
  text = text.replace(/^### (.*$)/gim, '<h4 style="margin:12px 0 6px;color:var(--accent)">$1</h4>')
             .replace(/^## (.*$)/gim, '<h3 style="margin:16px 0 8px;border-bottom:1px solid var(--border-color);padding-bottom:4px">$1</h3>')
             .replace(/^# (.*$)/gim, '<h2 style="margin:20px 0 10px">$1</h2>')
             .replace(/^\> (.*$)/gim, '<blockquote style="border-left:3px solid var(--accent);padding-left:10px;margin:8px 0;color:var(--text-secondary)">$1</blockquote>')
             .replace(/`([^`]+)`/g, '<code style="background:var(--bg-tertiary);padding:2px 4px;border-radius:3px;font-size:12px">$1</code>')
             .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
             .replace(/\*([^*]+)\*/g, '<em>$1</em>')
             .replace(/^\- (.*$)/gim, '<li style="margin-left:18px">$1</li>')
             .replace(/\n\n/g, '<br/><br/>');
  container.innerHTML = text;
}
