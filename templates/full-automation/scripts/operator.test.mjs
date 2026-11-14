import assert from 'node:assert/strict';
import { chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// Exercise real operator scripts with controlled local programs replacing
// Terraform, AWS, Docker, Corepack and the child Node commands. No credentials,
// service calls, dependency installs, image builds or AWS mutations occur.
const sourceDirectory = fileURLToPath(new URL('.', import.meta.url));
const digest = `sha256:${'a'.repeat(64)}`;
const identity = { clientId: 'test-client', clientSecret: 'test-only-client-secret-0123456789', sessionSecret: 's'.repeat(64) };
const fakeTool = `#!${process.execPath}\n` + String.raw`
const fs = require('node:fs');
const path = require('node:path');
const tool = path.basename(process.argv[1]);
const args = process.argv.slice(2);
const scenario = JSON.parse(fs.readFileSync(process.env.FAKE_SCENARIO, 'utf8'));
fs.appendFileSync(process.env.FAKE_LOG, JSON.stringify({ tool, args }) + '\n');
const digest = 'sha256:' + 'a'.repeat(64);
const stateFile = process.env.FAKE_STATE;
const state = fs.existsSync(stateFile) ? JSON.parse(fs.readFileSync(stateFile, 'utf8')) : {};
function answer(value) { process.stdout.write(value + '\n'); }
function fail(message) { process.stderr.write(message + '\n'); process.exitCode = 17; }
async function main() {
  if (tool === 'terraform') {
    const stack = args.some(arg => arg.endsWith('/infra/bootstrap')) ? 'bootstrap' : 'core';
    const command = args[1];
    if (command === 'init') {
      if (stack === 'bootstrap' && args.includes('-migrate-state')) {
        state.migrationAttempted = true;
        fs.writeFileSync(stateFile, JSON.stringify(state));
      }
      return;
    }
    if (command === 'apply') {
      if (scenario.failApplyStack === stack) return fail('simulated Terraform apply failure');
      return;
    }
    if (command === 'output') {
      const name = args.at(-1);
      if (name === 'state_bucket_arn') {
        if (scenario.migrationDeclined && state.migrationAttempted) return fail('no remote bootstrap outputs');
        return answer('arn:aws:s3:::test-state');
      }
      if (name === 'backend_hcl') {
        if (scenario.backendOutputFailure) { answer('partial backend configuration'); return fail('simulated output failure'); }
        return answer('bucket = "test-state"\nkey = "backstage/core.tfstate"\nregion = "us-east-1"\n');
      }
      const values = {
        aws_region: 'us-east-1', aws_account_id: '123456789012',
        ecr_repository_url: '123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage/portal',
        ecr_repository_name: 'backstage/portal',
        identity_secret_arn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:identity-example',
        approved_image_digest: scenario.approvedDigest ?? digest,
        rollout_state_machine_arn: 'arn:aws:states:us-east-1:123456789012:stateMachine:backstage-rollout',
        portal_url: 'https://backstage.example.com',
      };
      if (Object.hasOwn(values, name)) return answer(values[name]);
    }
    return fail('unexpected Terraform command');
  }
  if (tool === 'aws') {
    if (args.includes('get-caller-identity')) return answer(scenario.callerAccount ?? '123456789012');
    if (args.includes('describe-secret')) return answer(JSON.stringify(scenario.identityMissing ? {} : { version: ['AWSCURRENT'] }));
    if (args.includes('put-secret-value')) return answer('arn:aws:secretsmanager:us-east-1:123456789012:secret:identity-example');
    if (args.includes('get-login-password')) return answer('test-only-ecr-token');
    if (args.includes('describe-images')) return answer(scenario.publishedDigest ?? digest);
    if (args.includes('start-execution')) return answer('arn:aws:states:us-east-1:123456789012:execution:backstage-rollout:portal-test');
    if (args.includes('describe-execution')) return answer('SUCCEEDED');
    return fail('unexpected AWS command');
  }
  if (tool === 'docker') {
    if (args[0] === 'login') for await (const chunk of process.stdin) { /* consume the password pipe */ }
    return;
  }
  if (tool === 'corepack') return;
  if (tool === 'node' && (args[0]?.endsWith('check-owner.mjs') || args[0] === '--test')) return;
  return fail('unexpected external tool');
}
main().catch(error => { process.stderr.write(error.name + '\n'); process.exitCode = 18; });
`;

async function fixture(t, scenario = {}) {
  const root = await mkdtemp(path.join(tmpdir(), 'backstage-operator-test-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const directory of ['scripts', 'bin', 'infra/bootstrap', 'infra/core', 'app']) await mkdir(path.join(root, directory), { recursive: true });
  for (const name of await readdir(sourceDirectory)) {
    if (name.endsWith('.sh')) {
      const destination = path.join(root, 'scripts', name);
      await copyFile(path.join(sourceDirectory, name), destination);
      await chmod(destination, 0o700);
    }
  }
  for (const stack of ['bootstrap', 'core']) await writeFile(path.join(root, 'infra', stack, 'terraform.tfvars'), '# controlled test configuration\n');
  await writeFile(path.join(root, 'scenario.json'), JSON.stringify(scenario));
  await writeFile(path.join(root, 'events.ndjson'), '');
  const tool = path.join(root, 'bin', 'fake-tool.cjs');
  await writeFile(tool, fakeTool, { mode: 0o700 });
  for (const name of ['terraform', 'aws', 'docker', 'corepack', 'node']) await symlink(tool, path.join(root, 'bin', name));
  const secretPath = path.join(root, 'identity.json');
  await writeFile(secretPath, JSON.stringify(identity), { mode: 0o644 });
  return {
    root, secretPath,
    run(name, args = []) {
      return spawnSync('bash', [path.join(root, 'scripts', name), ...args], {
        cwd: root, encoding: 'utf8', timeout: 15_000,
        env: { ...process.env, PATH: `${path.join(root, 'bin')}:${process.env.PATH}`, FAKE_SCENARIO: path.join(root, 'scenario.json'), FAKE_LOG: path.join(root, 'events.ndjson'), FAKE_STATE: path.join(root, 'state.json') },
      });
    },
    async events() { return (await readFile(path.join(root, 'events.ndjson'), 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line)); },
  };
}

function called(events, tool, argument) {
  return events.some(event => event.tool === tool && event.args.includes(argument));
}

test('wrong-account credentials stop each AWS-mutating entry point before a mutation', async t => {
  for (const name of ['set-identity.sh', 'publish-image.sh', 'rollout.sh', 'deploy.sh']) {
    const f = await fixture(t, { callerAccount: '999999999999' });
    const result = f.run(name, name === 'set-identity.sh' ? [f.secretPath] : []);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /account mismatch/);
    const events = await f.events();
    assert.deepEqual(events.filter(event => event.tool === 'aws').map(event => event.args[3]), ['get-caller-identity']);
    assert.ok(!events.some(event => ['docker', 'corepack'].includes(event.tool)));
  }
});

test('identity upload passes only a file reference and restricts local permissions', async t => {
  const f = await fixture(t);
  const result = f.run('set-identity.sh', [f.secretPath]);
  assert.equal(result.status, 0, result.stderr);
  const events = await f.events();
  const upload = events.find(event => event.tool === 'aws' && event.args.includes('put-secret-value'));
  assert.equal(upload.args[upload.args.indexOf('--secret-string') + 1], `file://${f.secretPath}`);
  const argumentsText = JSON.stringify(events);
  assert.ok(!argumentsText.includes(identity.clientSecret));
  assert.ok(!argumentsText.includes(identity.sessionSecret));
  assert.equal((await stat(f.secretPath)).mode & 0o777, 0o600);
});

test('invalid or multiple identity JSON documents cannot be uploaded', async t => {
  for (const contents of [JSON.stringify({ ...identity, clientSecret: 'short' }), `${JSON.stringify(identity)}\n${JSON.stringify(identity)}`]) {
    const f = await fixture(t);
    await writeFile(f.secretPath, contents);
    assert.notEqual(f.run('set-identity.sh', [f.secretPath]).status, 0);
    assert.ok(!called(await f.events(), 'aws', 'put-secret-value'));
  }
});

test('deploy checks identity version metadata before building or applying', async t => {
  const f = await fixture(t, { identityMissing: true });
  const result = f.run('deploy.sh');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /AWSCURRENT/);
  const events = await f.events();
  assert.ok(called(events, 'aws', 'describe-secret'));
  assert.ok(!events.some(event => ['corepack', 'docker'].includes(event.tool)));
  assert.ok(!called(events, 'terraform', 'apply'));
  assert.ok(!called(events, 'aws', 'start-execution'));
});

test('successful deployment pins the pushed digest, applies it, then starts the workflow', async t => {
  const f = await fixture(t);
  const result = f.run('deploy.sh', ['test-release']);
  assert.equal(result.status, 0, result.stderr);
  const events = await f.events();
  const apply = events.findIndex(event => event.tool === 'terraform' && event.args.includes('apply'));
  const push = events.findIndex(event => event.tool === 'docker' && event.args[0] === 'push');
  const start = events.findIndex(event => event.tool === 'aws' && event.args.includes('start-execution'));
  assert.ok(push >= 0 && apply > push && start > apply);
  assert.ok(events[apply].args.includes('-var-file=image.auto.tfvars.json'));
  assert.deepEqual(JSON.parse(await readFile(path.join(f.root, 'infra/core/image.auto.tfvars.json'), 'utf8')), { image_digest: digest });
  assert.ok(!called(events, 'aws', 'update-service'), 'only the Step Functions workflow may activate ECS');
});

test('an apply failure or unexpected approved digest prevents workflow activation', async t => {
  for (const scenario of [{ failApplyStack: 'core' }, { approvedDigest: `sha256:${'b'.repeat(64)}` }]) {
    const f = await fixture(t, scenario);
    const result = f.run('deploy.sh', ['test-release']);
    assert.notEqual(result.status, 0);
    const events = await f.events();
    assert.ok(called(events, 'terraform', 'apply'));
    assert.ok(!called(events, 'aws', 'start-execution'));
  }
});

test('provision confirms migrated bootstrap state before initializing the core stack', async t => {
  const f = await fixture(t);
  const result = f.run('provision.sh');
  assert.equal(result.status, 0, result.stderr);
  const events = await f.events();
  const migration = events.findIndex(event => event.tool === 'terraform' && event.args.includes('-migrate-state'));
  const confirmed = events.findIndex((event, index) => index > migration && event.args.at(-1) === 'state_bucket_arn');
  const coreInit = events.findIndex(event => event.args[0]?.endsWith('/infra/core') && event.args.includes('init'));
  assert.ok(migration >= 0 && confirmed > migration && coreInit > confirmed);
  assert.ok(events[migration].args.includes('-backend-config=key=bootstrap/terraform.tfstate'));
  assert.match(await readFile(path.join(f.root, 'infra/bootstrap/backend.local.tf'), 'utf8'), /backend "s3"/);
  assert.ok(!events.some(event => event.tool === 'aws'), 'provisioning leaves activation to a later rollout');
});

test('declining migration stops before core initialization and prevents blind bootstrap re-apply', async t => {
  const f = await fixture(t, { migrationDeclined: true });
  assert.notEqual(f.run('provision.sh').status, 0);
  let events = await f.events();
  assert.ok(!events.some(event => event.args[0]?.endsWith('/infra/core') && event.args.includes('init')));
  const firstLength = events.length;
  assert.notEqual(f.run('provision.sh').status, 0);
  events = (await f.events()).slice(firstLength);
  assert.ok(!called(events, 'terraform', 'apply'));
});

test('failed backend output preserves an existing configuration instead of truncating it', async t => {
  const f = await fixture(t, { backendOutputFailure: true });
  const backend = path.join(f.root, 'infra/core/backend.local.hcl');
  await writeFile(backend, 'previous known backend configuration\n');
  assert.notEqual(f.run('provision.sh').status, 0);
  assert.equal(await readFile(backend, 'utf8'), 'previous known backend configuration\n');
  assert.deepEqual((await readdir(path.dirname(backend))).filter(name => name.startsWith('backend.local.hcl.')), []);
});
