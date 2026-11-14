import { spawnSync } from "node:child_process";
import {
  readFileSync,
  existsSync,
  mkdirSync,
  rmSync,
  writeFileSync,
  chmodSync,
} from "node:fs";
import { resolve } from "node:path";
import { randomBytes, randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { atomicJson } from "./config.mjs";

export function run(root, command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    timeout: 60_000,
    maxBuffer: 16 * 1024 * 1024,
    env: {
      ...process.env,
      AWS_PAGER: "",
      AWS_RETRY_MODE: "standard",
      AWS_MAX_ATTEMPTS: "3",
    },
    stdio: options.inherit ? "inherit" : "pipe",
    ...options,
  });
  if (result.error || result.status !== 0) {
    // Do not echo arguments or raw SDK responses: secret/file content may occur there.
    const error = new Error(
      `${command} ${args[0] ?? ""} failed (${result.error?.code ?? result.status ?? result.signal}); inspect the command's protected logs`,
    );
    error.serviceCode = result.stderr?.match(
      /An error occurred \(([A-Za-z0-9_.-]+)\)/,
    )?.[1];
    throw error;
  }
  return result.stdout ?? "";
}
export function context(root, config, target, injected = {}) {
  const invoke =
    injected.run ??
    ((command, args, options) => run(root, command, args, options));
  const aws = (service, operation, args = []) =>
    JSON.parse(
      invoke("aws", [
        "--region",
        config.awsRegion,
        service,
        operation,
        ...args,
        "--output",
        "json",
      ]) || "{}",
    );
  return {
    root,
    config,
    target,
    run: invoke,
    aws,
    delay: injected.delay ?? delay,
  };
}
export function verifyIdentity(ctx) {
  const identity = ctx.aws("sts", "get-caller-identity");
  if (
    identity.Account !== ctx.config.awsAccountId ||
    identity.Arn?.endsWith(":root")
  )
    throw new Error(
      "Wrong AWS account or root credentials. Use the intended account through SSO/assumed role.",
    );
  return { account: identity.Account, arn: identity.Arn };
}
export function targetFromOutputs(outputs) {
  const target = {};
  for (const key of [
    "aws_account_id",
    "aws_region",
    "ecr_repository_url",
    "ecr_repository_name",
    "identity_secret_arn",
    "application_secret_arns",
    "database_application_secret_arn",
    "database_admin_secret_arn",
    "database_identifier",
    "rollout_state_machine_arn",
    "rotation_state_machine_arn",
    "rollout_state_machine_version_arn",
    "ecs_cluster_arn",
    "ecs_service_name",
    "portal_task_definition_arn",
    "bootstrap_task_definition_arn",
    "portal_url",
    "release_lock_table",
    "target_group_arn",
    "alb_arn",
    "operations_topic_arn",
    "dashboard_name",
    "approved_image_digest",
    "kms_key_arn",
  ])
    target[key] = outputs[key]?.value;
  return target;
}
export function validateTarget(c, t) {
  if (
    !t ||
    t.aws_account_id !== c.awsAccountId ||
    t.aws_region !== c.awsRegion ||
    t.portal_url !== `https://${c.domainName}` ||
    t.ecs_service_name !== `${c.name}-portal` ||
    t.ecr_repository_name !== `${c.name}/portal` ||
    t.release_lock_table !== `${c.name}-release-lock`
  )
    throw new Error(
      "Deployment target does not match the reviewed account/configuration",
    );
  const prefix = `arn:aws:`;
  for (const key of [
    "identity_secret_arn",
    "database_application_secret_arn",
    "rollout_state_machine_arn",
    "ecs_cluster_arn",
    "portal_task_definition_arn",
  ]) {
    if (
      typeof t[key] !== "string" ||
      !t[key].startsWith(prefix) ||
      !t[key].includes(`:${c.awsRegion}:${c.awsAccountId}:`)
    )
      throw new Error(`Invalid target ${key}`);
  }
  if (
    t.ecr_repository_url !==
    `${c.awsAccountId}.dkr.ecr.${c.awsRegion}.amazonaws.com/${c.name}/portal`
  )
    throw new Error("Unexpected ECR destination");
  return t;
}
export function identityReady(ctx) {
  for (const arn of [
    ctx.target.identity_secret_arn,
    ...Object.values(ctx.target.application_secret_arns ?? {}),
  ]) {
    const secret = ctx.aws("secretsmanager", "describe-secret", [
      "--secret-id",
      arn,
    ]);
    if (
      !Object.values(secret.VersionIdsToStages ?? {}).some((stages) =>
        stages.includes("AWSCURRENT"),
      )
    )
      throw new Error(
        "An identity/plugin secret has no AWSCURRENT value. Set all configured values before deploying.",
      );
  }
}
export function writeIdentityTemplate(path) {
  mkdirSync(resolve(path, ".."), { recursive: true, mode: 0o700 });
  writeFileSync(
    path,
    JSON.stringify(
      {
        clientId: "REPLACE",
        clientSecret: "REPLACE",
        sessionSecret: randomBytes(48).toString("base64url"),
      },
      null,
      2,
    ) + "\n",
    { mode: 0o600, flag: "wx" },
  );
}
export function setIdentity(ctx, path) {
  let value;
  try { value = JSON.parse(readFileSync(path, 'utf8')); }
  catch { throw new Error('Identity file must contain one valid JSON object; no secret content is logged'); }
  if (
    !value ||
    typeof value.clientId !== "string" ||
    !/^[A-Za-z0-9_.-]{3,256}$/.test(value.clientId) ||
    value.clientId.includes("REPLACE") ||
    typeof value.clientSecret !== "string" ||
    value.clientSecret.length < 16 ||
    value.clientSecret.length > 1024 ||
    value.clientSecret.includes("REPLACE") ||
    typeof value.sessionSecret !== "string" ||
    !/^[A-Za-z0-9_-]{64,256}$/.test(value.sessionSecret)
  )
    throw new Error(
      "Invalid identity JSON. Supply the real OAuth client ID/secret and a strong generated session secret.",
    );
  chmodSync(path, 0o600);
  const receipt = ctx.aws("secretsmanager", "put-secret-value", [
    "--secret-id",
    ctx.target.identity_secret_arn,
    "--client-request-token",
    randomUUID(),
    "--secret-string",
    `file://${resolve(path)}`,
  ]);
  return {
    arn: receipt.ARN,
    versionId: receipt.VersionId,
    status: "complete",
    tasksReloaded: false,
  };
}
export function buildImage(ctx, tag) {
  if (!/^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$/.test(tag))
    throw new Error("Invalid immutable release tag");
  ctx.run("node", ["scripts/check-owner.mjs"]);
  ctx.run(
    "docker",
    [
      "build",
      "--platform",
      "linux/amd64",
      "--file",
      "packages/backend/Dockerfile",
      "--tag",
      `${ctx.target.ecr_repository_url}:${tag}`,
      ".",
    ],
    { cwd: resolve(ctx.root, "app"), inherit: true, timeout: 2_700_000 },
  );
  const password = ctx.run("aws", [
    "--region",
    ctx.config.awsRegion,
    "ecr",
    "get-login-password",
  ]);
  ctx.run(
    "docker",
    [
      "login",
      "--username",
      "AWS",
      "--password-stdin",
      ctx.target.ecr_repository_url.split("/")[0],
    ],
    { input: password },
  );
  ctx.run("docker", ["push", `${ctx.target.ecr_repository_url}:${tag}`], {
    inherit: true,
    timeout: 1_200_000,
  });
  const result = ctx.aws("ecr", "describe-images", [
    "--repository-name",
    ctx.target.ecr_repository_name,
    "--image-ids",
    `imageTag=${tag}`,
  ]);
  const digest = result.imageDetails?.[0]?.imageDigest;
  if (!/^sha256:[a-f0-9]{64}$/.test(digest ?? ""))
    throw new Error("ECR did not return an immutable image digest");
  atomicJson(resolve(ctx.root, "automation/local/image.json"), {
    imageDigest: digest,
    imageTag: tag,
    repository: ctx.target.ecr_repository_url,
  });
  return digest;
}
export async function scanImage(ctx, digest) {
  if (!/^sha256:[a-f0-9]{64}$/.test(digest ?? ""))
    throw new Error("Invalid scan digest");
  try {
    ctx.aws("ecr", "start-image-scan", [
      "--repository-name",
      ctx.target.ecr_repository_name,
      "--image-id",
      `imageDigest=${digest}`,
    ]);
  } catch (error) {
    if (
      !["LimitExceededException", "ScanAlreadyInProgressException"].includes(
        error.serviceCode,
      )
    )
      throw error;
  }
  for (let attempt = 0; attempt < 90; attempt++) {
    let result;
    try {
      result = ctx.aws("ecr", "describe-image-scan-findings", [
        "--repository-name",
        ctx.target.ecr_repository_name,
        "--image-id",
        `imageDigest=${digest}`,
      ]);
    } catch (error) {
      if (error.serviceCode !== "ScanNotFoundException") throw error;
      await ctx.delay(10_000);
      continue;
    }
    const status = result.imageScanStatus?.status;
    if (status === "COMPLETE" || status === "ACTIVE") {
      // ACTIVE is continuous enhanced scanning: a completed timestamp is still required.
      if (!result.imageScanFindings?.imageScanCompletedAt)
        throw new Error(
          "Scan has no completed finding timestamp; no clean scan assumed",
        );
      const age =
        Date.now() - Date.parse(result.imageScanFindings.imageScanCompletedAt);
      if (!Number.isFinite(age) || age < -60_000 || age > 86_400_000) {
        await ctx.delay(10_000);
        continue;
      }
      const counts = result.imageScanFindings.findingSeverityCounts ?? {};
      const critical = counts.CRITICAL ?? 0,
        high = counts.HIGH ?? 0;
      if (
        !Number.isInteger(critical) ||
        !Number.isInteger(high) ||
        critical > ctx.config.maximumCriticalFindings ||
        high > ctx.config.maximumHighFindings
      )
        throw new Error(
          `Image scan gate blocked: CRITICAL=${critical}, HIGH=${high}`,
        );
      return {
        status: "complete",
        digest,
        counts,
        scannedAt: result.imageScanFindings.imageScanCompletedAt,
      };
    }
    if (status !== "IN_PROGRESS" && status !== "PENDING")
      throw new Error(`Image scan is not usable: ${status ?? "unknown"}`);
    await ctx.delay(10_000);
  }
  throw new Error("Image scan did not finish within 90 observations");
}
export function serviceState(ctx) {
  const result = ctx.aws("ecs", "describe-services", [
    "--cluster",
    ctx.target.ecs_cluster_arn,
    "--services",
    ctx.target.ecs_service_name,
  ]);
  if (
    result.failures?.length ||
    result.services?.length !== 1 ||
    result.services[0].status !== "ACTIVE"
  )
    throw new Error(
      "Expected exactly one ACTIVE ECS service with no API failures",
    );
  return result.services[0];
}
export function assertUnlocked(ctx) {
  const lock = ctx.aws("dynamodb", "get-item", [
    "--table-name",
    ctx.target.release_lock_table,
    "--key",
    '{"LockId":{"S":"portal"}}',
    "--consistent-read",
  ]);
  if (lock.Item)
    throw new Error(
      `Release is locked by ${lock.Item.Owner?.S ?? "unknown owner"}; inspect it before changing infrastructure`,
    );
}
export async function snapshotDatabase(
  ctx,
  id = `${ctx.config.name}-release-${Date.now()}`,
) {
  if (
    !/^[a-z][a-z0-9-]{0,62}$/.test(id) ||
    id.endsWith("-") ||
    id.includes("--")
  )
    throw new Error("Invalid RDS snapshot identifier");
  ctx.aws("rds", "create-db-snapshot", [
    "--db-instance-identifier",
    ctx.target.database_identifier,
    "--db-snapshot-identifier",
    id,
    "--tags",
    `Key=Project,Value=${ctx.config.name}`,
  ]);
  for (let attempt = 0; attempt < 180; attempt++) {
    const s = ctx.aws("rds", "describe-db-snapshots", [
      "--db-snapshot-identifier",
      id,
    ]).DBSnapshots?.[0];
    if (s?.Status === "available")
      return {
        snapshotId: id,
        snapshotArn: s.DBSnapshotArn,
        status: "complete",
        database: ctx.target.database_identifier,
      };
    if (s?.Status !== "creating")
      throw new Error(`Snapshot is not ready: ${s?.Status ?? "unknown"}`);
    await ctx.delay(10_000);
  }
  throw new Error("Snapshot did not become available within 180 observations");
}
export async function runRollout(ctx, id, operation = "rollout") {
  if (!["rollout", "rotateDatabase"].includes(operation))
    throw new Error("Invalid workflow operation");
  const machine =
    operation === "rotateDatabase"
      ? ctx.target.rotation_state_machine_arn
      : ctx.target.rollout_state_machine_arn;
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(id)) throw new Error("Invalid request ID");
  const started = ctx.aws("stepfunctions", "start-execution", [
    "--state-machine-arn",
    machine,
    "--name",
    `portal-${id}`,
    "--input",
    JSON.stringify({ operation, requestId: id }),
  ]);
  if (typeof started.executionArn !== "string")
    throw new Error("No execution receipt returned");
  atomicJson(resolve(ctx.root, "automation/local/last-execution.json"), {
    executionArn: started.executionArn,
  });
  process.stdout.write(`Execution: ${started.executionArn}\n`);
  for (let attempt = 0; attempt < 150; attempt++) {
    const result = ctx.aws("stepfunctions", "describe-execution", [
      "--execution-arn",
      started.executionArn,
    ]);
    if (result.status === "SUCCEEDED") {
      const output = JSON.parse(result.output ?? "{}");
      if (
        output.status !== "SUCCEEDED" ||
        output.taskDefinitionArn !== ctx.target.portal_task_definition_arn
      )
        throw new Error(
          "Successful execution did not identify the expected task definition",
        );
      return {
        executionArn: started.executionArn,
        ...output,
        scope: "ecs-rollout-only",
      };
    }
    if (result.status !== "RUNNING")
      throw new Error(
        `Execution ${result.status ?? "unknown"}: ${started.executionArn}. Inspect before recovery; no automatic database downgrade attempted.`,
      );
    await ctx.delay(10_000);
  }
  throw new Error(
    `Local wait timed out; remote execution continues: ${started.executionArn}`,
  );
}
export function recoverLock(ctx, owner) {
  if (
    typeof owner !== "string" ||
    ![
      `${ctx.config.name}-portal-rollout`,
      `${ctx.config.name}-database-rotation`,
    ].some((name) =>
      owner.startsWith(
        `arn:aws:states:${ctx.config.awsRegion}:${ctx.config.awsAccountId}:execution:${name}:`,
      ),
    )
  )
    throw new Error("Supply the exact lock-owner execution ARN");
  const execution = ctx.aws("stepfunctions", "describe-execution", [
    "--execution-arn",
    owner,
  ]);
  if (
    !["SUCCEEDED", "FAILED", "TIMED_OUT", "ABORTED"].includes(execution.status)
  )
    throw new Error("Execution is still active or its status is unknown");
  const service = serviceState(ctx);
  if (
    service.pendingCount !== 0 ||
    service.deployments?.some((x) => x.rolloutState === "IN_PROGRESS")
  )
    throw new Error(
      "ECS still has an in-flight deployment; resolve it before releasing the lock",
    );
  for (const family of [
    `${ctx.config.name}-db-bootstrap`,
    `${ctx.config.name}-db-rotate`,
  ]) {
    const tasks = ctx.aws("ecs", "list-tasks", [
      "--cluster",
      ctx.target.ecs_cluster_arn,
      "--family",
      family,
      "--desired-status",
      "RUNNING",
    ]);
    if (!Array.isArray(tasks.taskArns) || tasks.taskArns.length)
      throw new Error(
        "A bootstrap/rotation task is active or unknown; stop and inspect it before lock recovery",
      );
  }
  ctx.aws("dynamodb", "delete-item", [
    "--table-name",
    ctx.target.release_lock_table,
    "--key",
    '{"LockId":{"S":"portal"}}',
    "--condition-expression",
    "#owner = :owner",
    "--expression-attribute-names",
    '{"#owner":"Owner"}',
    "--expression-attribute-values",
    JSON.stringify({ ":owner": { S: owner } }),
  ]);
  return { status: "complete", recoveredExecutionArn: owner };
}
export async function withOperatorLock(root, fn) {
  const directory = resolve(root, ".operator-lock");
  try {
    mkdirSync(directory, { mode: 0o700 });
  } catch {
    throw new Error(
      "Another local operator command holds .operator-lock. Inspect its owner before manual recovery.",
    );
  }
  writeFileSync(
    resolve(directory, "owner.json"),
    JSON.stringify({ pid: process.pid, startedAt: new Date().toISOString() }),
  );
  try {
    return await fn();
  } finally {
    rmSync(directory, { recursive: true });
  }
}
