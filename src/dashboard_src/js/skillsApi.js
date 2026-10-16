/**
 * API client for Skill editing, content retrieval, and staging operations.
 */

export async function fetchSkillContent(name) {
  const res = await fetch(`/api/skills/content?name=${encodeURIComponent(name)}`);
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    throw new Error(err.error || `Failed to fetch skill content (${res.status})`);
  }
  return res.json();
}

export async function saveSkillContent(name, content) {
  const res = await fetch('/api/skills/save', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, content }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    throw new Error(err.error || `Failed to save skill (${res.status})`);
  }
  return res.json();
}

export async function revertSkillContent(name) {
  const res = await fetch('/api/skills/revert', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    throw new Error(err.error || `Failed to revert skill (${res.status})`);
  }
  return res.json();
}
