// API calls for Tool Micro-Rules management

export async function fetchToolRules(project) {
  const q = project && project !== 'all' ? `?project=${encodeURIComponent(project)}` : '?project=all';
  const resp = await fetch(`/api/tool-rules${q}`);
  if (!resp.ok) throw new Error('Failed to fetch tool rules: ' + resp.statusText);
  return resp.json();
}

export async function saveToolRules(project, rules) {
  const q = project && project !== 'all' ? `?project=${encodeURIComponent(project)}` : '?project=all';
  const resp = await fetch(`/api/tool-rules${q}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ rules }),
  });
  if (!resp.ok) throw new Error('Failed to save tool rules: ' + resp.statusText);
  return resp.json();
}

export async function revertToolRules(project) {
  if (!project || project === 'all') throw new Error('Project path required to revert');
  const resp = await fetch(`/api/tool-rules/revert?project=${encodeURIComponent(project)}`, {
    method: 'POST',
  });
  if (!resp.ok) throw new Error('Failed to revert tool rules: ' + resp.statusText);
  return resp.json();
}
