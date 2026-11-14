import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import test from 'node:test';

// `npm ci --prefix infra/tools --ignore-scripts` installs this test-only evaluator.
// JSONATA_MODULE can optionally point to an isolated installation. AWS documents 2.0.6:
// https://docs.aws.amazon.com/step-functions/latest/dg/transforming-data.html
// These tests evaluate real rendered expressions. They do not emulate AWS
// service integrations, IAM, ECS timing, or the full Step Functions runtime.
const require = createRequire(import.meta.url);
const evaluator = process.env.JSONATA_MODULE ?? 'jsonata';
const jsonata = require(evaluator);
const evaluatorVersion = require(`${evaluator}/package.json`).version;
const published = JSON.parse(await readFile(new URL('../core/tests/fixtures/portal-rollout-published.asl.json', import.meta.url), 'utf8'));
const unpublished = JSON.parse(await readFile(new URL('../core/tests/fixtures/portal-rollout-infrastructure-only.asl.json', import.meta.url), 'utf8'));
const approvedTask = published.States.DeployPortal.Arguments.TaskDefinition;
const desired = published.States.DeployPortal.Arguments.DesiredCount;
const deploymentId = 'ecs-svc/expected-rollout';

function expression(value) {
  assert.equal(typeof value, 'string');
  assert.ok(value.startsWith('{%') && value.endsWith('%}'));
  return jsonata(value.slice(2, -2));
}

async function evaluate(value, input = {}, variables = {}, result) {
  const compiled = expression(value);
  compiled.assign('states', { input, result });
  for (const [name, content] of Object.entries(variables)) compiled.assign(name, content);
  return compiled.evaluate({});
}

async function choose(name, input, variables = {}, workflow = published) {
  const state = workflow.States[name];
  assert.equal(state.Type, 'Choice');
  for (const choice of state.Choices) {
    const result = await evaluate(choice.Condition, input, variables);
    assert.equal(typeof result, 'boolean', `${name} must produce a Boolean condition`);
    if (result) return choice.Next;
  }
  return state.Default;
}

function stableService() {
  return {
    Failures: [],
    Services: [{
      Status: 'ACTIVE', DesiredCount: desired, RunningCount: desired, PendingCount: 0,
      Deployments: [{ Id: deploymentId, Status: 'PRIMARY', TaskDefinition: approvedTask, RolloutState: 'COMPLETED' }],
    }],
  };
}

test('uses the JSONata version documented by AWS', () => {
  assert.equal(evaluatorVersion, '2.0.6');
});

test('every rendered JSONata expression compiles in both deployment variants', () => {
  let count = 0;
  function inspect(value) {
    if (typeof value === 'string' && value.startsWith('{%')) {
      expression(value);
      count += 1;
    } else if (value && typeof value === 'object') {
      for (const item of Object.values(value)) inspect(item);
    }
  }
  inspect(published);
  inspect(unpublished);
  assert.ok(count > 0);
});

test('request validation accepts bounded IDs and safely rejects malformed data', async () => {
  for (const requestId of ['a', '_', 'same-request_42', 'a'.repeat(64)]) {
    assert.equal(await choose('ValidateRequest', { operation: 'rollout', requestId }), 'RememberRequest');
  }
  const invalid = [
    undefined, null, false, 7, [], {},
    { operation: 'rollout' },
    ...['', 'a'.repeat(65), 'space here', 'newline\n', 'é', 42, null, true, [], {}].map(requestId => ({ operation: 'rollout', requestId })),
    { operation: 'destroy', requestId: 'valid' },
    { requestId: 'valid' },
  ];
  for (const input of invalid) assert.equal(await choose('ValidateRequest', input), 'InvalidRequest');
});

test('an infrastructure-only stack fails before running its placeholder image', async () => {
  assert.equal(await choose('ValidateRequest', { operation: 'rollout', requestId: 'valid' }, {}, unpublished), 'ImageNotPublished');
});

test('bootstrap success requires exactly one named container with numeric exit zero', async () => {
  const good = { Name: 'db-bootstrap', ExitCode: 0 };
  for (const Containers of [[good], [{ Name: 'sidecar', ExitCode: 0 }, good]]) {
    assert.equal(await choose('CheckBootstrapExit', { Containers }), 'DeployPortal');
  }
  for (const input of [
    {}, { Containers: [] }, { Containers: [good, good] },
    { Containers: [{ Name: 'other', ExitCode: 0 }] },
    ...[1, -1, '0', null, undefined].map(ExitCode => ({ Containers: [{ Name: 'db-bootstrap', ExitCode }] })),
  ]) assert.equal(await choose('CheckBootstrapExit', input), 'BootstrapContainerFailed');
});

test('deployment receipt selects PRIMARY by status and captures its ID', async () => {
  const primary = { Id: deploymentId, Status: 'PRIMARY', TaskDefinition: approvedTask };
  const secondary = { Id: 'old', Status: 'ACTIVE', TaskDefinition: `${approvedTask}-old` };
  const input = { Service: { Deployments: [secondary, primary] } };
  assert.equal(await choose('ValidateDeploymentReceipt', input), 'RememberDeployment');
  assert.equal(await evaluate(published.States.RememberDeployment.Assign.deploymentId, input), deploymentId);
  for (const Deployments of [[], [secondary], [primary, primary], [{ ...primary, TaskDefinition: `${approvedTask}-other` }]]) {
    assert.equal(await choose('ValidateDeploymentReceipt', { Service: { Deployments } }), 'InvalidDeploymentReceipt');
  }
  assert.equal(await choose('ValidateDeploymentReceipt', {}), 'InvalidDeploymentReceipt');
});

test('service observation rejects API failures, absent services and inactive services', async () => {
  const valid = stableService();
  assert.equal(await choose('ValidateServiceResponse', valid), 'CheckDeployment');
  assert.equal(await choose('ValidateServiceResponse', { Services: valid.Services }), 'CheckDeployment');
  for (const input of [
    {}, { Failures: [], Services: [] },
    { ...valid, Failures: [{ Reason: 'MISSING' }] },
    { ...valid, Services: [...valid.Services, ...valid.Services] },
    { Failures: [], Services: [{ ...valid.Services[0], Status: 'DRAINING' }] },
  ]) assert.equal(await choose('ValidateServiceResponse', input), 'DescribeServiceFailed');
});

test('only the exact completed deployment and expected counts can succeed', async () => {
  const variables = { deploymentId, attempts: 0 };
  assert.equal(await choose('CheckDeployment', stableService(), variables), 'ReleaseSuccessfulLock');
  const mutations = [
    service => { service.DesiredCount = desired + 1; },
    service => { service.RunningCount = 0; },
    service => { service.PendingCount = 1; },
    service => { service.Deployments[0].Id = 'rollback-or-other-deployment'; },
    service => { service.Deployments[0].Status = 'ACTIVE'; },
    service => { service.Deployments[0].TaskDefinition = `${approvedTask}-old`; },
    service => { service.Deployments[0].RolloutState = 'IN_PROGRESS'; },
    service => { delete service.Deployments[0].RolloutState; },
    service => { service.Deployments.push({ Id: 'old', Status: 'ACTIVE', RolloutState: 'COMPLETED' }); },
    service => { service.Deployments = []; },
  ];
  for (const mutate of mutations) {
    const input = stableService();
    mutate(input.Services[0]);
    assert.equal(await choose('CheckDeployment', input, variables), 'IncrementAttempts');
  }
  const failed = stableService();
  failed.Services[0].Deployments[0].RolloutState = 'FAILED';
  assert.equal(await choose('CheckDeployment', failed, variables), 'DeploymentFailed');
  failed.Services[0].Deployments[0].Id = 'unrelated-failed-deployment';
  assert.equal(await choose('CheckDeployment', failed, variables), 'IncrementAttempts');
});

test('a deployment that never stabilizes stops at the sixtieth observation', async () => {
  const input = stableService();
  input.Services[0].Deployments[0].RolloutState = 'IN_PROGRESS';
  let attempts = published.States.RememberDeployment.Assign.attempts;
  let polls = 0;
  let next;
  for (; polls < 65; polls += 1) {
    next = await choose('CheckDeployment', input, { deploymentId, attempts });
    if (next !== 'IncrementAttempts') break;
    attempts = await evaluate(published.States.IncrementAttempts.Assign.attempts, input, { attempts });
  }
  assert.equal(next, 'RolloutTimedOut');
  assert.equal(polls + 1, 60);
  assert.equal(await choose('CheckDeployment', stableService(), { deploymentId, attempts: 59 }), 'ReleaseSuccessfulLock');
});

test('saved correlation and final output use validated state and approved resource constants', async () => {
  const input = { operation: 'rollout', requestId: 'accepted-request', taskDefinition: 'attacker-selected', command: 'unapproved' };
  const requestId = await evaluate(published.States.RememberRequest.Assign.requestId, input);
  assert.equal(requestId, 'accepted-request');
  const output = published.States.RememberRequest.Output;
  assert.deepEqual(Object.keys(output).sort(), ['operation', 'requestId']);
  assert.equal(await evaluate(output.operation, input), 'rollout');
  assert.equal(await evaluate(output.requestId, input), requestId);
  const final = published.States.RolloutSucceeded.Output;
  assert.equal(await evaluate(final.requestId, {}, { requestId }), requestId);
  assert.equal(await evaluate(final.deploymentId, {}, { deploymentId }), deploymentId);
  assert.equal(final.taskDefinitionArn, approvedTask);
  assert.equal(final.status, 'SUCCEEDED');
});
