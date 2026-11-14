import { UserInfoService } from '@backstage/backend-plugin-api';
import {
  AuthorizeResult,
  createPermission,
} from '@backstage/plugin-permission-common';
import { PolicyQueryUser } from '@backstage/plugin-permission-node';
import {
  actionExecutePermission,
  taskCreatePermission,
  taskReadPermission,
  templateDryRunPermission,
} from '@backstage/plugin-scaffolder-common/alpha';
import {
  OPERATOR_GROUP,
  PlatformPermissionPolicy,
  ROLLOUT_ACTION_ID,
} from './permissions';

const user = { credentials: {} } as PolicyQueryUser;
const catalogRead = createPermission({
  name: 'catalog.entity.read',
  attributes: { action: 'read' },
});
const catalogWrite = createPermission({
  name: 'catalog.entity.create',
  attributes: { action: 'create' },
});

function policyFor(groups: string[]) {
  const userInfo: UserInfoService = {
    getUserInfo: async () => ({
      userEntityRef: 'user:default/alice',
      ownershipEntityRefs: groups,
    }),
  };
  return new PlatformPermissionPolicy(userInfo);
}

describe('platform permission policy', () => {
  it('denies callers without a user identity', async () => {
    expect(
      await policyFor([OPERATOR_GROUP]).handle({
        permission: taskCreatePermission,
      }),
    ).toEqual({ result: AuthorizeResult.DENY });
    expect(await policyFor([]).handle({ permission: catalogRead })).toEqual({
      result: AuthorizeResult.DENY,
    });
  });

  it('permits catalog reads but denies rollout submission and task logs to ordinary members', async () => {
    const policy = policyFor(['group:default/developers']);
    expect(await policy.handle({ permission: catalogRead }, user)).toEqual({
      result: AuthorizeResult.ALLOW,
    });
    for (const permission of [
      taskCreatePermission,
      taskReadPermission,
      actionExecutePermission,
    ]) {
      expect(await policy.handle({ permission }, user)).toEqual({
        result: AuthorizeResult.DENY,
      });
    }
  });

  it('permits operators to submit tasks and execute only the fixed rollout action', async () => {
    const policy = policyFor([OPERATOR_GROUP]);
    expect(
      await policy.handle({ permission: taskCreatePermission }, user),
    ).toEqual({ result: AuthorizeResult.ALLOW });
    expect(
      await policy.handle({ permission: actionExecutePermission }, user),
    ).toMatchObject({
      result: AuthorizeResult.CONDITIONAL,
      conditions: {
        rule: 'HAS_ACTION_ID',
        params: { actionId: ROLLOUT_ACTION_ID },
      },
    });
  });

  it('denies inline templates, catalog mutation, and unknown privileges even to operators', async () => {
    const policy = policyFor([OPERATOR_GROUP]);
    const unknown = createPermission({
      name: 'new.plugin.admin',
      attributes: { action: 'create' },
    });
    for (const permission of [
      templateDryRunPermission,
      catalogWrite,
      unknown,
    ]) {
      expect(await policy.handle({ permission }, user)).toEqual({
        result: AuthorizeResult.DENY,
      });
    }
  });
});
