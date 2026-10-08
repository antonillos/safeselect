const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const workflow = fs.readFileSync(path.join(__dirname, '../workflows/smart-merge.yml'), 'utf8');
const script = workflow.split('          script: |\n')[1].split('\n').map(line => line.replace(/^            /, '')).join('\n');

async function run(options = {}) {
  const events = [];
  const pull = { number: 1, title: 'ci: migrate releases', user: { login: options.author || 'antonillos' }, state: 'open', draft: false, mergeable: true, mergeable_state: 'clean', head: { sha: 'abc', ref: 'feature', repo: { full_name: 'antonillos/safeselect' } }, base: { ref: 'develop' }, changed_files: 1 };
  const github = {
    rest: {
      issues: { createComment: async () => {} },
      reactions: { createForIssueComment: async () => {} },
      repos: {
        getCollaboratorPermissionLevel: async () => ({ data: { permission: options.permission || 'write' } }),
        getCommit: async () => ({ data: { commit: { verification: { verified: true } } } }),
      },
      pulls: {
        get: async () => ({ data: events.includes('approve') && options.changed ? { ...pull, head: { ...pull.head, sha: 'changed' } } : pull }),
        listCommits: 'commits',
        createReview: async review => { events.push('approve'); assert.equal(review.commit_id, 'abc'); if (options.approvalFailure) throw new Error('denied'); },
      },
      actions: {
        createWorkflowDispatch: async () => {},
        listWorkflowRuns: async () => ({ data: { workflow_runs: [{ id: 2, created_at: new Date().toISOString(), head_sha: 'abc', status: 'completed', conclusion: 'success' }] } }),
        listJobsForWorkflowRun: 'jobs',
      },
      git: { deleteRef: async () => {} },
    },
    request: async route => { assert.ok(route.includes('/stacks')); return { data: [] }; },
    paginate: async endpoint => endpoint === 'jobs'
      ? [{ name: 'Verify', status: 'completed', conclusion: options.checkFailure ? 'failure' : 'success' }]
      : (events.push('signatures'), [{ sha: 'abc', commit: { verification: { verified: !options.unsigned } } }]),
  };
  const sandbox = {
    github, context: { repo: { owner: 'antonillos', repo: 'safeselect' }, issue: { number: 1 }, actor: 'antonillos', payload: { comment: { body: '/merge', id: 1 } } },
    core: { setFailed: () => events.push('failed'), info: () => {}, warning: () => {} },
    process: { env: { MERGE_APP_TOKEN: 'mock' } },
    getOctokit: token => { assert.equal(token, 'mock'); return { request: async (route, args) => { events.push('merge'); assert.equal(args.expected_head_sha, 'abc'); return { status: 200, data: { status: 'merged', details: { sha: 'result' } } }; } }; },
    setTimeout: fn => { fn(); },
  };
  await vm.runInNewContext(`(async () => {${script}\n})()`, sandbox);
  return events;
}

test('author may request approval by distinct workflow identity; App merges exact SHA', async () => {
  assert.deepEqual(await run(), ['signatures', 'approve', 'merge']);
});
for (const [name, options] of Object.entries({ unauthorized: { permission: 'read' }, unsigned: { unsigned: true }, approvalDenied: { approvalFailure: true }, changedHead: { changed: true }, failedChecks: { checkFailure: true } })) {
  test(`${name} fails closed without merge`, async () => {
    const events = await run(options);
    assert.ok(!events.includes('merge'));
    assert.ok(events.includes('failed'));
  });
}
