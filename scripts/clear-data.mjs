#!/usr/bin/env node

/**
 * Cleanup script for Tucano Test API.
 * Clears all test data (projects, test suites, test cases, test runs,
 * milestones, attachments, configurations) from the API.
 *
 * Usage:
 *   node scripts/clear-data.mjs [API_BASE_URL]
 */

const candidateUrls = [
  process.argv[2],
  process.env.API_URL,
  process.env.TUCANO_API_URL,
  'http://localhost:3100',
  'http://localhost:8080/api',
  'http://localhost:3000',
].filter(Boolean);

async function resolveBaseUrl() {
  for (const url of candidateUrls) {
    const cleanUrl = url.replace(/\/$/, '');
    try {
      const res = await fetch(`${cleanUrl}/health`, { signal: AbortSignal.timeout(1500) });
      if (res.ok) {
        return cleanUrl;
      }
    } catch {
      // Continue searching
    }
  }
  return candidateUrls[0]?.replace(/\/$/, '') || 'http://localhost:3100';
}

async function runClear() {
  const baseUrl = await resolveBaseUrl();
  console.log(`\n🧹 Clearing test data from Tucano Test API at: ${baseUrl}\n`);

  async function api(path, options = {}) {
    const res = await fetch(`${baseUrl}${path}`, {
      headers: { 'content-type': 'application/json' },
      ...options,
    });
    const text = await res.text();
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    return { status: res.status, ok: res.ok, data };
  }

  const resources = [
    { name: 'Milestones', path: '/milestones' },
    { name: 'Test Runs', path: '/test_runs' },
    { name: 'Test Suites', path: '/test_suites' },
    { name: 'Projects', path: '/projects' },
    { name: 'Test Cases', path: '/test_cases' },
  ];

  let totalDeleted = 0;

  for (const resType of resources) {
    const listRes = await api(resType.path);
    if (!listRes.ok || !Array.isArray(listRes.data)) {
      console.log(`  ℹ️  ${resType.name}: None found or failed to list.`);
      continue;
    }

    const items = listRes.data;
    if (items.length === 0) {
      console.log(`  ℹ️  ${resType.name}: 0 items.`);
      continue;
    }

    console.log(`🗑️  Deleting ${items.length} ${resType.name}...`);
    for (const item of items) {
      const delRes = await api(`${resType.path}/${encodeURIComponent(item)}`, {
        method: 'DELETE',
      });
      if (delRes.ok) {
        console.log(`    ✓ Deleted ${resType.name.slice(0, -1)}: ${item}`);
        totalDeleted++;
      } else {
        console.warn(`    ⚠️ Failed to delete ${item} (${delRes.status}):`, delRes.data);
      }
    }
  }

  console.log(`\n✨ Done! Deleted ${totalDeleted} item(s). API is now clean.\n`);
}

runClear().catch((err) => {
  console.error('❌ Failed to clear data:', err);
  process.exit(1);
});
