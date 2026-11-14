import {
  DescribeExecutionCommand,
  SFNClient,
  StartExecutionCommand,
} from '@aws-sdk/client-sfn';
import { createMockActionContext } from '@backstage/plugin-scaffolder-node-test-utils';
import {
  createRolloutAction,
  parseStateMachineArn,
  rolloutRequest,
} from './awsRollout';

const stateMachineArn =
  'arn:aws:states:us-east-1:123456789012:stateMachine:portal-rollout';
const taskId = 'f119114c-8a4e-4ce6-b37b-698b3e550e00';

function executionArnFor(id: string) {
  return `arn:aws:states:us-east-1:123456789012:execution:portal-rollout:${
    rolloutRequest(id).name
  }`;
}

describe('rollout request boundaries', () => {
  it('uses a deterministic safe name and a fixed operation', () => {
    const first = rolloutRequest(taskId, '  Planned restart  ');
    expect(first.name).toMatch(/^rollout-[a-f0-9]{48}$/);
    expect(first.name).toBe(rolloutRequest(taskId, 'another reason').name);
    expect(first.name).not.toBe(rolloutRequest('different-task').name);
    expect(JSON.parse(first.input)).toEqual({
      operation: 'rollout',
      requestId: taskId,
      reason: 'Planned restart',
    });
  });

  it('enforces the state machine request ID length contract', () => {
    expect(() => rolloutRequest('a'.repeat(64))).not.toThrow();
    expect(() => rolloutRequest('a'.repeat(65))).toThrow();
    for (const invalid of ['', 'a/b', 'a:b', 'a\n']) {
      expect(() => rolloutRequest(invalid)).toThrow();
    }
  });

  it('rejects unbounded and control-character reasons', () => {
    expect(() => rolloutRequest(taskId, 'a'.repeat(160))).not.toThrow();
    expect(() => rolloutRequest(taskId, 'a'.repeat(161))).toThrow();
    expect(() => rolloutRequest(taskId, '\n')).toThrow();
    expect(() => rolloutRequest(taskId, 'line\nbreak')).toThrow();
  });

  it('rejects qualified ARNs and malformed targets', () => {
    expect(parseStateMachineArn(stateMachineArn).region).toBe('us-east-1');
    expect(() => parseStateMachineArn(`${stateMachineArn}:1`)).toThrow();
    expect(() =>
      parseStateMachineArn(
        'arn:aws:states:us-east-1:123456789012:execution:other:x',
      ),
    ).toThrow();
    expect(() => parseStateMachineArn('https://example.org')).toThrow();
  });
});

describe('rollout action', () => {
  const mockSend = () =>
    jest.fn(async (_command: unknown) => ({} as Record<string, unknown>));

  it('submits only the configured workflow and immediately returns an execution link', async () => {
    const send = mockSend().mockResolvedValue({
      executionArn: executionArnFor(taskId),
    });
    const action = createRolloutAction({
      stateMachineArn,
      client: { send } as unknown as Pick<SFNClient, 'send'>,
    });
    const context = createMockActionContext({
      input: { reason: 'Deploy approved image' },
      task: { id: taskId },
    });
    await action.handler(context);

    expect(send).toHaveBeenCalledTimes(1);
    const command = send.mock.calls[0][0] as StartExecutionCommand;
    expect(command).toBeInstanceOf(StartExecutionCommand);
    expect(command.input.stateMachineArn).toBe(stateMachineArn);
    expect(command.input).toMatchObject(
      rolloutRequest(taskId, 'Deploy approved image'),
    );
    expect(context.output).toHaveBeenCalledWith(
      'executionArn',
      executionArnFor(taskId),
    );
    expect(context.output).toHaveBeenCalledWith(
      'consoleUrl',
      expect.stringContaining('console.aws.amazon.com/states/'),
    );
    expect(action.supportsDryRun).toBe(false);
  });

  it('reconciles a retried submission after the original execution already finished', async () => {
    const alreadyExists = Object.assign(new Error('Already exists'), {
      name: 'ExecutionAlreadyExists',
    });
    const send = mockSend()
      .mockRejectedValueOnce(alreadyExists)
      .mockResolvedValueOnce({
        stateMachineArn,
        input: rolloutRequest(taskId).input,
        status: 'SUCCEEDED',
      });
    const action = createRolloutAction({
      stateMachineArn,
      client: { send } as unknown as Pick<SFNClient, 'send'>,
    });
    const context = createMockActionContext({
      input: {},
      task: { id: taskId },
    });
    await action.handler(context);
    expect(send).toHaveBeenCalledTimes(2);
    expect(send.mock.calls[1][0]).toBeInstanceOf(DescribeExecutionCommand);
    expect(context.output).toHaveBeenCalledWith(
      'executionArn',
      executionArnFor(taskId),
    );
  });

  it('does not accept an existing execution with different inputs', async () => {
    const alreadyExists = Object.assign(new Error('Already exists'), {
      name: 'ExecutionAlreadyExists',
    });
    const send = mockSend()
      .mockRejectedValueOnce(alreadyExists)
      .mockResolvedValueOnce({ stateMachineArn, input: '{}' });
    const action = createRolloutAction({
      stateMachineArn,
      client: { send } as unknown as Pick<SFNClient, 'send'>,
    });
    const context = createMockActionContext({
      input: {},
      task: { id: taskId },
    });
    await expect(action.handler(context)).rejects.toThrow('does not match');
    expect(context.output).not.toHaveBeenCalled();
  });

  it('propagates authorization failures and never launches an alternate workflow', async () => {
    const send = mockSend().mockRejectedValue(
      new Error('AccessDeniedException'),
    );
    const action = createRolloutAction({
      stateMachineArn,
      client: { send } as unknown as Pick<SFNClient, 'send'>,
    });
    const context = createMockActionContext({
      input: {},
      task: { id: taskId },
    });
    await expect(action.handler(context)).rejects.toThrow(
      'AccessDeniedException',
    );
    expect(send).toHaveBeenCalledTimes(1);
    expect(context.output).not.toHaveBeenCalled();
  });
});
