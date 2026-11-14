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

function policyFor(groups: string[], userEntityRef = 'user:default/alice') {
  const userInfo: UserInfoService = {
    getUserInfo: async () => ({
      userEntityRef,
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

  it('permits catalog reads but denies task submission and task logs to ordinary members', async () => {
    const policy = policyFor(['group:default/developers']);
    expect(await policy.handle({ permission: catalogRead }, user)).toEqual({
      result: AuthorizeResult.ALLOW,
    });
    for (const permission of [taskCreatePermission, taskReadPermission]) {
      expect(await policy.handle({ permission }, user)).toEqual({
        result: AuthorizeResult.DENY,
      });
    }
  });

  it('gives guests member reads without operator or admin privileges', async () => {
    const guestPolicy = policyFor([], 'user:default/guest');
    const memberPolicy = policyFor(['group:default/developers']);
    const readable = [
      catalogRead,
      createPermission({
        name: 'scaffolder.template.parameter.read',
        attributes: { action: 'read' },
      }),
      createPermission({
        name: 'scaffolder.template.step.read',
        attributes: { action: 'read' },
      }),
    ];
    for (const permission of readable) {
      expect(
        await guestPolicy.handle({ permission }, user),
      ).toEqual(await memberPolicy.handle({ permission }, user));
      expect(await guestPolicy.handle({ permission }, user)).toEqual({
        result: AuthorizeResult.ALLOW,
      });
    }
    for (const permission of [
      taskCreatePermission,
      taskReadPermission,
      actionExecutePermission,
      createPermission({
        name: 'backstage.admin',
        attributes: { action: 'create' },
      }),
    ]) {
      expect(await guestPolicy.handle({ permission }, user)).toEqual({
        result: AuthorizeResult.DENY,
      });
    }
  });

  it('permits operators to submit and read scaffolder tasks', async () => {
    const policy = policyFor([OPERATOR_GROUP]);
    expect(
      await policy.handle({ permission: taskCreatePermission }, user),
    ).toEqual({ result: AuthorizeResult.ALLOW });
    expect(
      await policy.handle({ permission: taskReadPermission }, user),
    ).toEqual({ result: AuthorizeResult.ALLOW });
  });

  it('denies inline templates, catalog mutation, and unknown privileges even to operators', async () => {
    const policy = policyFor([OPERATOR_GROUP]);
    const unknown = createPermission({
      name: 'new.plugin.admin',
      attributes: { action: 'create' },
    });
    for (const permission of [
      actionExecutePermission,
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
