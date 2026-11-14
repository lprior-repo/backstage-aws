import { createHash } from 'node:crypto';
import {
  DescribeExecutionCommand,
  SFNClient,
  StartExecutionCommand,
} from '@aws-sdk/client-sfn';
import {
  coreServices,
  createBackendModule,
} from '@backstage/backend-plugin-api';
import { InputError } from '@backstage/errors';
import {
  createTemplateAction,
  scaffolderActionsExtensionPoint,
} from '@backstage/plugin-scaffolder-node';
import { ROLLOUT_ACTION_ID } from './permissions';

type StepFunctionsClient = Pick<SFNClient, 'send'>;

export function parseStateMachineArn(stateMachineArn: string) {
  const match =
    /^arn:aws:states:([a-z0-9-]+):(\d{12}):stateMachine:([A-Za-z0-9_-]{1,80})$/.exec(
      stateMachineArn,
    );
  if (!match) {
    throw new InputError(
      'awsBackstage.rolloutStateMachineArn must be an unqualified commercial-AWS state machine ARN',
    );
  }
  return { region: match[1], accountId: match[2], stateMachineName: match[3] };
}

export function rolloutRequest(taskId: string, reason?: string) {
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(taskId)) {
    throw new InputError('The scaffolder task must have a valid task ID');
  }
  if (
    reason !== undefined &&
    (reason.length > 160 || /[\u0000-\u001f\u007f]/.test(reason))
  ) {
    throw new InputError(
      'Reason must contain at most 160 printable characters',
    );
  }
  const normalizedReason = reason?.trim();
  const name = `rollout-${createHash('sha256')
    .update(taskId)
    .digest('hex')
    .slice(0, 48)}`;
  const input = JSON.stringify({
    operation: 'rollout',
    requestId: taskId,
    ...(normalizedReason ? { reason: normalizedReason } : {}),
  });
  return { name, input };
}

/** Fixed ARN, fixed operation, stable execution name; templates cannot select IAM targets. */
export function createRolloutAction(options: {
  stateMachineArn: string;
  client: StepFunctionsClient;
}) {
  const { stateMachineArn, client } = options;
  const { region, accountId, stateMachineName } =
    parseStateMachineArn(stateMachineArn);

  return createTemplateAction({
    id: ROLLOUT_ACTION_ID,
    description:
      'Request the configured Backstage database initialization and ECS rollout workflow',
    supportsDryRun: false,
    schema: {
      input: {
        reason: z =>
          z
            .string()
            .max(160)
            .regex(/^[^\u0000-\u001f\u007f]*$/)
            .optional(),
      },
      output: {
        executionArn: z => z.string(),
        consoleUrl: z => z.string().url(),
      },
    },
    async handler(ctx) {
      const { name, input } = rolloutRequest(ctx.task.id, ctx.input.reason);
      const executionArn = `arn:aws:states:${region}:${accountId}:execution:${stateMachineName}:${name}`;

      try {
        const result = await client.send(
          new StartExecutionCommand({ stateMachineArn, name, input }),
          { abortSignal: AbortSignal.timeout(15_000) },
        );
        if (result.executionArn !== executionArn) {
          throw new Error(
            'Step Functions returned an unexpected execution ARN',
          );
        }
      } catch (error) {
        if (
          !(error instanceof Error) ||
          error.name !== 'ExecutionAlreadyExists'
        ) {
          throw error;
        }
        // Standard workflows reject reuse once complete. Reconcile a retried
        // submission against its original input rather than starting a new run.
        const existing = await client.send(
          new DescribeExecutionCommand({ executionArn }),
          { abortSignal: AbortSignal.timeout(15_000) },
        );
        if (
          existing.input !== input ||
          existing.stateMachineArn !== stateMachineArn
        ) {
          throw new Error(
            'Existing rollout execution does not match this request',
          );
        }
      }

      const consoleUrl = `https://${region}.console.aws.amazon.com/states/home?region=${region}#/v2/executions/details/${encodeURIComponent(
        executionArn,
      )}`;
      ctx.logger.info(`Submitted rollout execution ${executionArn}`);
      ctx.output('executionArn', executionArn);
      ctx.output('consoleUrl', consoleUrl);
      // Do not wait here: this workflow deliberately replaces the portal task.
    },
  });
}

export const awsRolloutModule = createBackendModule({
  pluginId: 'scaffolder',
  moduleId: 'aws-rollout',
  register(reg) {
    reg.registerInit({
      deps: {
        scaffolder: scaffolderActionsExtensionPoint,
        config: coreServices.rootConfig,
        lifecycle: coreServices.lifecycle,
      },
      async init({ scaffolder, config, lifecycle }) {
        const stateMachineArn = config.getString(
          'awsBackstage.rolloutStateMachineArn',
        );
        const { region } = parseStateMachineArn(stateMachineArn);
        const client = new SFNClient({
          region,
          maxAttempts: 3,
          retryMode: 'standard',
        });
        lifecycle.addShutdownHook(async () => client.destroy());
        scaffolder.addActions(createRolloutAction({ stateMachineArn, client }));
      },
    });
  },
});
