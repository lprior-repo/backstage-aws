import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  mkdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import {
  context,
  verifyIdentity,
  scanImage,
  recoverLock,
  runRollout,
  setIdentity,
  validateTarget,
  writeIdentityTemplate,
} from "./runtime.mjs";
import { validateConfig, configure } from "./config.mjs";
import { validateLive, classifyRelease, jsonataOnly } from "./validation.mjs";
const c = {
  awsAccountId: "123456789012",
  awsRegion: "us-east-1",
  name: "backstage",
  environment: "production",
  domainName: "backstage.example.com",
  route53ZoneId: "Z0123456789",
  allowedIngressCidrs: ["203.0.113.10/32"],
  githubRepository: "example/platform",
  ownerGithubId: "12345",
  budgetEmail: "owner@example.com",
  alertEmail: "owner@example.com",
  monthlyBudgetUsd: 300,
  accountBaseline: true,
  singleNatGateway: false,
  databaseMultiAz: true,
  portalDesiredCount: 2,
  maximumCriticalFindings: 0,
  maximumHighFindings: 0,
};
const arn = (service, resource) =>
  `arn:aws:${service}:us-east-1:123456789012:${resource}`;
const target = {
  aws_account_id: c.awsAccountId,
  aws_region: c.awsRegion,
  portal_url: `https://${c.domainName}`,
  ecs_service_name: "backstage-portal",
  ecr_repository_name: "backstage/portal",
  ecr_repository_url:
    "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage/portal",
  release_lock_table: "backstage-release-lock",
  identity_secret_arn: arn("secretsmanager", "secret:identity"),
  database_application_secret_arn: arn("secretsmanager", "secret:db-app"),
  rollout_state_machine_arn: arn(
    "states",
    "stateMachine:backstage-portal-rollout",
  ),
  rotation_state_machine_arn: arn(
    "states",
    "stateMachine:backstage-database-rotation",
  ),
  ecs_cluster_arn: arn("ecs", "cluster/backstage-cluster"),
  portal_task_definition_arn: arn("ecs", "task-definition/backstage-portal:7"),
};
const digest = "sha256:" + "a".repeat(64);
function fixture(t, dispatch) {
  const root = mkdtempSync(resolve(tmpdir(), "backstage-automation-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const calls = [];
  return {
    root,
    calls,
    ctx: context(root, c, target, {
      delay: async () => {},
      run: (command, args, options) => {
        calls.push({ command, args, options });
        return JSON.stringify(dispatch(args[2], args[3], args, options));
      },
    }),
  };
}
test("configuration rejects wrong types, open ingress, injection and reserved secret names", () => {
  assert.equal(validateConfig(c), c);
  for (const change of [
    { awsAccountId: "1;echo unsafe" },
    { allowedIngressCidrs: ["0.0.0.0/0"] },
    { allowedIngressCidrs: ["999.2.3.4/32"] },
    { domainName: "https://example.com" },
    { accountBaseline: "true" },
    { portalDesiredCount: 0 },
    { maximumHighFindings: -1 },
    { clientSecret: 'must-not-enter-nonsecret-config' },
    { additionalSecrets: { token: { environmentVariable: 'GITHUB_TOKEN', value: 'not-a-reference' } } },
    { ownerGithubId: "REPLACE" },
    {
      additionalSecrets: {
        token: { environmentVariable: "POSTGRES_PASSWORD" },
      },
    },
  ])
    assert.throws(() => validateConfig({ ...c, ...change }));
});
test("configure creates references and config only; it does not write secret values", (t) => {
  const { root } = fixture(t, () => ({}));
  mkdirSync(resolve(root, "app/catalog"), { recursive: true });
  writeFileSync(
    resolve(root, "app/catalog/org.yaml"),
    "github.com/user-id: 'REPLACE_WITH_GITHUB_NUMERIC_ID'\n",
  );
  configure(root, {
    ...c,
    additionalSecrets: {
      token: { environmentVariable: "GITHUB_TOKEN", jsonKey: "token" },
    },
  });
  assert.match(
    readFileSync(resolve(root, "app/catalog/org.yaml"), "utf8"),
    /'12345'/,
  );
  const core = JSON.parse(
    readFileSync(resolve(root, "infra/core/terraform.tfvars.json"), "utf8"),
  );
  assert.deepEqual(core.application_secrets.token, {
    environment_variable: "GITHUB_TOKEN",
    json_key: "token",
    description: "Backstage plugin credential",
  });
  assert.equal(core.image_digest, undefined);
});
test("account mismatch and root credentials fail before any AWS mutation", (t) => {
  for (const identity of [
    { Account: "999999999999", Arn: arn("iam", "user/operator") },
    { Account: c.awsAccountId, Arn: "arn:aws:iam::123456789012:root" },
  ]) {
    const { ctx, calls } = fixture(t, () => identity);
    assert.throws(() => verifyIdentity(ctx));
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args[3], "get-caller-identity");
  }
});
test("target rejects a cross-account manifest or altered ECR destination", () => {
  assert.equal(validateTarget(c, target), target);
  for (const changes of [
    { aws_account_id: "111111111111" },
    { ecr_repository_url: "malicious.example/image" },
    { portal_url: "http://backstage.example.com" },
    {
      identity_secret_arn: arn("secretsmanager", "secret:identity").replace(
        c.awsAccountId,
        "999999999999",
      ),
    },
  ])
    assert.throws(() => validateTarget(c, { ...target, ...changes }));
});
test("identity template is private and refuses overwrite", (t) => {
  const { root } = fixture(t, () => ({})),
    path = resolve(root, ".secrets/identity.json");
  writeIdentityTemplate(path);
  assert.equal(statSync(path).mode & 0o777, 0o600);
  assert.match(
    JSON.parse(readFileSync(path, "utf8")).sessionSecret,
    /^[A-Za-z0-9_-]{64}$/,
  );
  assert.throws(() => writeIdentityTemplate(path));
});
test("identity upload uses a file reference and returns only secret-version metadata", (t) => {
  const { ctx, root, calls } = fixture(t, () => ({
      ARN: target.identity_secret_arn,
      VersionId: "version-1",
    })),
    path = resolve(root, "secret.json");
  const secret = {
    clientId: "client-id",
    clientSecret: "private-value-do-not-log",
    sessionSecret: "s".repeat(64),
  };
  writeFileSync(path, JSON.stringify(secret));
  const receipt = setIdentity(ctx, path);
  assert.equal(receipt.tasksReloaded, false);
  assert.equal(receipt.versionId, "version-1");
  const argv = calls[0].args;
  assert.ok(argv.includes(`file://${path}`));
  assert.ok(!argv.join(" ").includes(secret.clientSecret));
  assert.equal(statSync(path).mode & 0o777, 0o600);
  writeFileSync(path, 'private-value-do-not-log is not JSON');
  assert.throws(() => setIdentity(ctx, path), error => !error.message.includes('private-value-do-not-log'));
  assert.equal(calls.length, 1, 'malformed secret must not cause an upload');
});
test("scan blocks high findings, unknown status, missing timestamp and stale results", async (t) => {
  const good = {
    imageScanStatus: { status: "COMPLETE" },
    imageScanFindings: {
      imageScanCompletedAt: new Date().toISOString(),
      findingSeverityCounts: { HIGH: 0, CRITICAL: 0 },
    },
  };
  for (const result of [
    {
      ...good,
      imageScanFindings: {
        ...good.imageScanFindings,
        findingSeverityCounts: { HIGH: 1 },
      },
    },
    { imageScanStatus: { status: "FAILED" } },
    { imageScanStatus: { status: "COMPLETE" }, imageScanFindings: {} },
    {
      ...good,
      imageScanFindings: {
        ...good.imageScanFindings,
        imageScanCompletedAt: "2020-01-01T00:00:00Z",
      },
    },
  ]) {
    const { ctx } = fixture(t, (_s, op) =>
      op === "start-image-scan" ? {} : result,
    );
    await assert.rejects(scanImage(ctx, digest));
  }
  const { ctx } = fixture(t, (_s, op) =>
    op === "start-image-scan" ? {} : good,
  );
  assert.equal((await scanImage(ctx, digest)).status, "complete");
});
test("scan retry is bounded and only known not-yet-published scan errors are retried", async (t) => {
  let observations = 0;
  const { ctx } = fixture(t, (_s, op) => {
    if (op === "start-image-scan") return {};
    observations++;
    const e = new Error("not found");
    e.serviceCode = "ScanNotFoundException";
    throw e;
  });
  await assert.rejects(scanImage(ctx, digest), /90 observations/);
  assert.equal(observations, 90);
});
test("lock recovery refuses active executions and does not delete another owners lock", (t) => {
  const owner = arn("states", "execution:backstage-portal-rollout:test");
  const active = fixture(t, () => ({ status: "RUNNING" }));
  assert.throws(() => recoverLock(active.ctx, owner), /still active/);
  assert.equal(active.calls.length, 1);
  const f = fixture(t, (_s, op) => {
    if (op === "describe-execution") return { status: "FAILED" };
    if (op === "describe-services")
      return {
        services: [
          {
            status: "ACTIVE",
            pendingCount: 0,
            deployments: [{ rolloutState: "COMPLETED" }],
          },
        ],
      };
    if (op === "list-tasks") return { taskArns: [] };
    return {};
  });
  recoverLock(f.ctx, owner);
  const last = f.calls.at(-1).args;
  assert.ok(last.includes("#owner = :owner"));
  assert.ok(last.includes(JSON.stringify({ ":owner": { S: owner } })));
});
test("rollout refuses a nominally successful execution for the wrong task revision", async (t) => {
  const f = fixture(t, (_s, op) =>
    op === "start-execution"
      ? {
          executionArn: arn(
            "states",
            "execution:backstage-portal-rollout:test",
          ),
        }
      : {
          status: "SUCCEEDED",
          output: JSON.stringify({
            status: "SUCCEEDED",
            taskDefinitionArn: "wrong",
          }),
        },
  );
  await assert.rejects(
    runRollout(f.ctx, "request"),
    /expected task definition/,
  );
});
test("rotation starts only the maintenance state machine with an explicit operation", async (t) => {
  const f = fixture(t, (_s, op) =>
    op === "start-execution"
      ? {
          executionArn: arn(
            "states",
            "execution:backstage-database-rotation:test",
          ),
        }
      : {
          status: "SUCCEEDED",
          output: JSON.stringify({
            status: "SUCCEEDED",
            taskDefinitionArn: target.portal_task_definition_arn,
          }),
        },
  );
  await runRollout(f.ctx, "request", "rotateDatabase");
  const start = f.calls[0].args;
  assert.ok(start.includes(target.rotation_state_machine_arn));
  assert.ok(
    start.includes(
      JSON.stringify({ operation: "rotateDatabase", requestId: "request" }),
    ),
  );
});
test("partial live validation never claims full release/security success", async (t) => {
  const f = fixture(t, (_s, op) => {
    if (op === "describe-services")
      return {
        services: [
          {
            status: "ACTIVE",
            taskDefinition: target.portal_task_definition_arn,
            runningCount: 2,
            pendingCount: 0,
            deployments: [{ rolloutState: "COMPLETED" }],
          },
        ],
      };
    if (op === "describe-target-health")
      return {
        TargetHealthDescriptions: [
          { TargetHealth: { State: "healthy" } },
          { TargetHealth: { State: "healthy" } },
        ],
      };
    if (op === "describe-db-instances")
      return {
        DBInstances: [
          {
            PubliclyAccessible: false,
            StorageEncrypted: true,
            BackupRetentionPeriod: 14,
            DeletionProtection: true,
            DBInstanceStatus: "available",
          },
        ],
      };
    if (op === "list-subscriptions-by-topic")
      return {
        Subscriptions: [
          {
            Protocol: "email",
            Endpoint: c.alertEmail,
            SubscriptionArn: "arn:confirmed",
          },
        ],
      };
    return {};
  });
  const result = await validateLive(f.ctx, undefined, async (url) => ({
    status: url.includes("/api/catalog/") ? 401 : 200,
  }));
  assert.equal(result.status, "needs verification");
  assert.ok(
    result.checks.some(
      (x) =>
        x.name.includes("Interactive") && x.status === "needs verification",
    ),
  );
  assert.equal(classifyRelease([{ status: "unknown" }]), "needs verification");
  assert.equal(classifyRelease([{ status: "blocked" }]), "blocked");
});
test("JSONata contract rejects JSONPath at every nested level", () => {
  assert.equal(jsonataOnly({ QueryLanguage: "JSONata", States: {} }), true);
  for (const value of [
    { QueryLanguage: "JSONPath" },
    { QueryLanguage: "JSONata", States: { X: { Parameters: {} } } },
    { QueryLanguage: "JSONata", States: { X: { QueryLanguage: "JSONPath" } } },
    {
      QueryLanguage: "JSONata",
      States: { X: { Arguments: { "Value.$": "$.x" } } },
    },
  ])
    assert.throws(() => jsonataOnly(value));
});
