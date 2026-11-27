import { el, escapeHtml } from './dom.js';
import { fetchProjects, pruneProjects, deleteProject, setSelectedProject, getSelectedProject, triggerAutoCleanup, fetchData, fetchGraphData } from './api.js';
import { renderGraphView } from './render/graphView.js';
import { showConfirm, showAlert } from './dialog.js';
import { createProjectCombobox } from './combobox.js';
import { t } from './i18n/index.js';

let headerCombobox = null;
let latestProjects = [];
let controlsBound = false;

async function loadAndRenderGraph() {
  const data = await fetchGraphData({ view: 'files' });
  renderGraphView(data);
}

function updateProjectBadges(val) {
  const badge = el('project-status-badge');
  const headerBadge = el('header-project-badge');
  const chosen = latestProjects.find(p => p.path === val);
  const isMissing = chosen && chosen.status === 'missing';
  const msg = isMissing
    ? t('sidebar.missing_msg')
    : (val === 'all' ? t('sidebar.all_tracked') : t('sidebar.active_project', { name: chosen?.name || val }));

  if (badge) {
    badge.textContent = isMissing ? t('sidebar.missing_on_disk') : '';
    badge.style.display = isMissing ? 'block' : 'none';
    badge.style.color = '#ffbd2e';
  }
  if (headerBadge) {
    headerBadge.textContent = msg;
    headerBadge.style.display = 'inline-block';
    headerBadge.style.color = isMissing ? '#ffbd2e' : 'var(--accent-primary)';
  }
}

function handleProjectChange(val) {
  const select = el('project-selector');
  const headerSelect = el('header-project-selector');
  if (select) select.value = val;
  if (headerSelect) headerSelect.value = val;
  if (headerCombobox) headerCombobox.setValue(val);
  setSelectedProject(val);
  updateProjectBadges(val);

  fetchData({ force: true });
  if (location.hash.slice(1) === 'graph') {
    loadAndRenderGraph();
  } else if (location.hash.slice(1) === 'rules') {
    import('./render/rulesView.js').then(m => m.renderRulesView());
  }
}

async function handleProjectDelete(pPath, pName) {
  const ok = await showConfirm({
    title: t('dialog.delete_project_title'),
    message: t('dialog.delete_project_msg', { name: pName || pPath }),
    subtext: t('dialog.delete_project_subtext'),
    confirmText: t('dialog.delete_project_btn'),
    cancelText: t('dialog.cancel'),
    variant: 'danger',
  });
  if (!ok) return;

  const res = await deleteProject(pPath, true);
  await showAlert({
    title: t('dialog.delete_project_title'),
    message: res.message || t('dialog.delete_project_success', { name: pName || pPath }),
    variant: res.success ? 'success' : 'danger',
  });

  if (getSelectedProject() === pPath) {
    handleProjectChange('all');
  }
  await initProjectSelector();
  fetchData({ force: true });
}

export async function initProjectSelector() {
  const select = el('project-selector');
  const headerSelect = el('header-project-selector');
  const pruneBtn = el('btn-prune-projects');
  if (!select) return;

  const projects = await fetchProjects();
  const optionsHtml = [`<option value="all">${escapeHtml(t('sidebar.all_projects_analytics'))}</option>`];
  const seenPaths = new Set();
  const uniqueProjects = [];

  projects.forEach(p => {
    if (p.path && !seenPaths.has(p.path)) {
      seenPaths.add(p.path);
      uniqueProjects.push(p);
      const isMissing = p.status === 'missing';
      const label = `${p.name || p.path} ${isMissing ? `(${t('sidebar.missing')})` : ''}`.trim();
      optionsHtml.push(`<option value="${escapeHtml(p.path)}" ${isMissing ? 'style="color: #ffbd2e;"' : ''}>${escapeHtml(label)}</option>`);
    }
  });

  latestProjects = uniqueProjects;
  const fullHtml = optionsHtml.join('');
  select.innerHTML = fullHtml;
  if (headerSelect) headerSelect.innerHTML = fullHtml;

  const currentVal = getSelectedProject() || 'all';
  select.value = currentVal;
  if (headerSelect) headerSelect.value = currentVal;
  updateProjectBadges(currentVal);

  const inputEl = el('header-project-search');
  const dropdownEl = el('header-combobox-dropdown');
  const optionsEl = el('header-combobox-options');
  const arrowEl = el('header-combobox-arrow');

  if (inputEl && dropdownEl && optionsEl) {
    if (headerCombobox) {
      headerCombobox.setValue(currentVal);
      headerCombobox.updateProjects(uniqueProjects);
    } else {
      headerCombobox = createProjectCombobox({
        inputEl,
        dropdownEl,
        optionsEl,
        arrowEl,
        projects: uniqueProjects,
        selectedVal: currentVal,
        onSelect: handleProjectChange,
        onDelete: handleProjectDelete,
      });
    }
  }

  if (!controlsBound) {
    controlsBound = true;

    select.addEventListener('change', () => handleProjectChange(select.value));
    if (headerSelect) {
      headerSelect.addEventListener('change', () => handleProjectChange(headerSelect.value));
    }

    if (pruneBtn) {
      pruneBtn.addEventListener('click', async () => {
        const ok = await showConfirm({
          title: t('dialog.prune_title'),
          message: t('dialog.prune_msg'),
          subtext: t('dialog.prune_subtext'),
          confirmText: t('dialog.prune_btn'),
          cancelText: t('dialog.cancel'),
          variant: 'danger',
        });
        if (ok) {
          const res = await pruneProjects();
          await showAlert({
            title: t('dialog.prune_title'),
            message: res.message || t('dialog.pruned_success', { count: res.pruned_count || 0 }),
            variant: 'success',
          });
          await initProjectSelector();
          fetchData({ force: true });
        }
      });
    }

    const cleanupBtn = el('btn-auto-cleanup');
    if (cleanupBtn) {
      cleanupBtn.addEventListener('click', async () => {
        const ok = await showConfirm({
          title: t('dialog.optimize_title'),
          message: t('dialog.optimize_msg'),
          subtext: t('dialog.optimize_subtext'),
          confirmText: t('dialog.optimize_btn'),
          cancelText: t('dialog.cancel'),
          variant: 'info',
        });
        if (!ok) return;

        const res = await triggerAutoCleanup();
        await showAlert({
          title: t('dialog.optimize_title'),
          message: res.message || t('dialog.cleanup_complete'),
          variant: res.success ? 'success' : 'danger',
        });
        fetchData({ force: true });
      });
    }
  }

  if (currentVal === 'all') {
    const defaultProj = el('sidebar-proj')?.textContent?.trim();
    if (defaultProj && defaultProj !== '--') {
      const match = uniqueProjects.find(p => p.path === defaultProj || p.name === defaultProj);
      if (match) {
        handleProjectChange(match.path);
      }
    }
  }
}
