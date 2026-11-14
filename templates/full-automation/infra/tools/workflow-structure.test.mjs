import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const directory = join(dirname(fileURLToPath(import.meta.url)), '..', 'core', 'tests', 'fixtures');
const load = mode => JSON.parse(readFileSync(join(directory, `portal-rollout-${mode}.asl.json`), 'utf8'));

function destinations(state) {
  return [state.Next, state.Default, ...(state.Choices ?? []).map(choice => choice.Next), ...(state.Catch ?? []).map(handler => handler.Next)].filter(Boolean);
}

for (const mode of ['published', 'infrastructure-only']) {
  test(`${mode}: every transition resolves and every reachable path has a valid state shape`, () => {
    const machine = load(mode);
    assert.equal(machine.QueryLanguage, 'JSONata');
    assert.equal(machine.TimeoutSeconds, 1200);
    assert.ok(machine.States[machine.StartAt]);
    const visited = new Set();
    const queue = [machine.StartAt];
    while (queue.length > 0) {
      const name = queue.shift();
      if (visited.has(name)) continue;
      visited.add(name);
      const state = machine.States[name];
      assert.ok(['Choice', 'Pass', 'Task', 'Wait', 'Succeed', 'Fail'].includes(state.Type), `${name} type`);
      if (['Succeed', 'Fail'].includes(state.Type)) {
        assert.equal(destinations(state).length, 0, `${name} terminal state`);
      } else {
        assert.ok(destinations(state).length > 0, `${name} needs a destination`);
      }
      if (state.Type === 'Choice') assert.ok(state.Default, `${name} must fail or continue explicitly`);
      if (state.Type === 'Task') {
        assert.ok(state.TimeoutSeconds > 0 && state.TimeoutSeconds <= 300, `${name} bounded time`);
        assert.ok(state.Catch?.some(handler => handler.ErrorEquals.includes('States.ALL')), `${name} failure path`);
        for (const retry of state.Retry ?? []) {
          assert.ok(retry.MaxAttempts <= 2, `${name} bounded retries`);
          assert.ok(!retry.ErrorEquals.includes('States.ALL'), `${name} no indiscriminate retry`);
        }
      }
      for (const destination of destinations(state)) {
        assert.ok(machine.States[destination], `${name} -> ${destination} must resolve`);
        queue.push(destination);
      }
    }
    if (mode === 'infrastructure-only') {
      assert.ok(visited.has('ImageNotPublished'));
      assert.ok(!visited.has('BootstrapDatabase'), 'An unpublished image cannot launch any task.');
    } else {
      assert.ok(visited.has('RolloutSucceeded'));
      assert.ok(visited.has('BootstrapContainerFailed'));
      assert.ok(visited.has('DeploymentFailed'));
      assert.ok(visited.has('RolloutTimedOut'));
    }
  });
}

test('all ECS request arguments are fixed configuration, never execution input or expressions', () => {
  const machine = load('published');
  const visit = value => {
    if (typeof value === 'string') {
      assert.ok(!value.includes('{%'), `Dynamic AWS argument: ${value}`);
      assert.ok(!value.includes('$states'), `Execution-dependent AWS argument: ${value}`);
    } else if (Array.isArray(value)) {
      value.forEach(visit);
    } else if (value && typeof value === 'object') {
      Object.values(value).forEach(visit);
    }
  };
  for (const state of Object.values(machine.States)) {
    if (state.Type === 'Task' && state.Resource.includes(':ecs:')) visit(state.Arguments);
  }
  const bootstrap = machine.States.BootstrapDatabase.Arguments;
  assert.equal(bootstrap.EnableExecuteCommand, false);
  assert.equal(bootstrap.Count, 1);
  assert.equal(bootstrap.LaunchType, 'FARGATE');
  assert.equal(bootstrap.NetworkConfiguration.AwsvpcConfiguration.AssignPublicIp, 'DISABLED');
  assert.equal(bootstrap.Overrides, undefined);
  const update = machine.States.DeployPortal.Arguments;
  assert.equal(update.DesiredCount, 2);
  assert.equal(update.ForceNewDeployment, true);
  assert.match(update.TaskDefinition, /:task-definition\/backstage-portal:[0-9]+$/u);
  assert.match(bootstrap.TaskDefinition, /:task-definition\/backstage-db-bootstrap:[0-9]+$/u);
});

test('bootstrap success precedes deployment; maintenance drain is a separate fixed service mutation', () => {
  const machine = load('published');
  const mutationStates = Object.entries(machine.States).filter(([, state]) => state.Resource?.endsWith('ecs:updateService'));
  assert.equal(mutationStates.length, 2);
  assert.equal(machine.States.DrainPortal.Arguments.DesiredCount, 0);
  assert.equal(machine.States.SelectOperation.Choices[0].Next, 'DrainPortal');
  assert.equal(machine.States.BootstrapDatabase.Next, 'CheckBootstrapExit');
  assert.equal(machine.States.CheckBootstrapExit.Choices[0].Next, 'DeployPortal');
  assert.equal(machine.States.CheckBootstrapExit.Default, 'BootstrapContainerFailed');
  assert.equal(machine.States.IncrementAttempts.Next, 'WaitForService');
  assert.equal(machine.States.WaitForService.Seconds, 10);
  assert.equal(machine.States.CheckDeployment.Choices.at(-1).Next, 'RolloutTimedOut');
});
