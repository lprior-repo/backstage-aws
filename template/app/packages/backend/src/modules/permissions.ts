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
  taskCreatePermission,
  taskReadPermission,
} from '@backstage/plugin-scaffolder-common/alpha';

export const OPERATOR_GROUP = 'group:default/platform-operators';

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

    // Includes catalog mutation, inline template dry runs, and cancellation.
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
