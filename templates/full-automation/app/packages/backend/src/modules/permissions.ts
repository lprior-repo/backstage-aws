import {
  coreServices,
  createBackendModule,
  UserInfoService,
} from '@backstage/backend-plugin-api';
import {
  AuthorizeResult,
  isPermission,
  PolicyDecision,
} from '@backstage/plugin-permission-common';
import {
  PermissionPolicy,
  PolicyQuery,
  PolicyQueryUser,
} from '@backstage/plugin-permission-node';
import { policyExtensionPoint } from '@backstage/plugin-permission-node/alpha';
import {
  actionExecutePermission,
  taskCreatePermission,
  taskReadPermission,
} from '@backstage/plugin-scaffolder-common/alpha';
import {
  createScaffolderActionConditionalDecision,
  scaffolderActionConditions,
} from '@backstage/plugin-scaffolder-backend/alpha';

export const OPERATOR_GROUP = 'group:default/platform-operators';
export const ROLLOUT_ACTION_ID = 'aws:backstage:rollout';

const readablePermissions = new Set([
  'catalog.entity.read',
  'scaffolder.template.parameter.read',
  'scaffolder.template.step.read',
]);

/** Catalog changes and unrecognized permissions require an explicit code change. */
export class PlatformPermissionPolicy implements PermissionPolicy {
  constructor(private readonly userInfo: UserInfoService) {}

  async handle(
    request: PolicyQuery,
    user?: PolicyQueryUser,
  ): Promise<PolicyDecision> {
    if (!user) {
      return { result: AuthorizeResult.DENY };
    }

    if (readablePermissions.has(request.permission.name)) {
      return { result: AuthorizeResult.ALLOW };
    }

    const { ownershipEntityRefs } = await this.userInfo.getUserInfo(
      user.credentials,
    );
    if (!ownershipEntityRefs.includes(OPERATOR_GROUP)) {
      return { result: AuthorizeResult.DENY };
    }

    if (
      isPermission(request.permission, taskCreatePermission) ||
      isPermission(request.permission, taskReadPermission)
    ) {
      return { result: AuthorizeResult.ALLOW };
    }

    if (isPermission(request.permission, actionExecutePermission)) {
      return createScaffolderActionConditionalDecision(
        request.permission,
        scaffolderActionConditions.hasActionId({ actionId: ROLLOUT_ACTION_ID }),
      );
    }

    // Includes catalog mutation, inline template dry runs, and cancellation.
    // Cancelling a scaffolder task would not cancel its external AWS execution.
    return { result: AuthorizeResult.DENY };
  }
}

export const platformPermissionModule = createBackendModule({
  pluginId: 'permission',
  moduleId: 'platform-policy',
  register(reg) {
    reg.registerInit({
      deps: {
        policy: policyExtensionPoint,
        userInfo: coreServices.userInfo,
      },
      async init({ policy, userInfo }) {
        policy.setPolicy(new PlatformPermissionPolicy(userInfo));
      },
    });
  },
});
