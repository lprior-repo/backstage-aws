#!/usr/bin/env node
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  existsSync,
  rmSync,
  chmodSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { atomicJson, configure, loadConfig } from "./config.mjs";
import {
  run,
  context,
  verifyIdentity,
  targetFromOutputs,
  validateTarget,
  identityReady,
  writeIdentityTemplate,
  setIdentity,
  buildImage,
  scanImage,
  serviceState,
  assertUnlocked,
  snapshotDatabase,
  runRollout,
  recoverLock,
  withOperatorLock,
} from "./runtime.mjs";
import { validateLive, jsonataOnly } from "./validation.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const help = `Backstage AWS automation (Node + AWS CLI + Terraform + Docker)

./platform configure --config deployment.json
./platform doctor --config deployment.json
./platform secrets-init --file .secrets/github-identity.json
./platform up --identity .secrets/github-identity.json
./platform export-target
./platform build --target deployment-target.json [--tag UNIQUE_TAG]
./platform scan --digest sha256:...
./platform release [--digest sha256:...] [--tag UNIQUE_TAG]
./platform rollback --digest sha256:... --database-compatible
./platform snapshot
./platform rollout
./platform secrets-set --identity .secrets/github-identity.json
./platform secrets-put --name plugin-name --file .secrets/value.json
./platform secrets-rotate --identity .secrets/github-identity.json
./platform db-rotate --maintenance-window
./platform validate-live
./platform validate-workflow
./platform recover-lock --execution EXECUTION_ARN
./platform latest
./platform upgrade [--version 1.56.5]
./platform github-setup
./platform restore-drill --snapshot SNAPSHOT_ID --new-id UNIQUE_DB_IDENTIFIER

All commands accept --config FILE. Build/scan can use a nonsecret --target FILE.
Release and rollback show a Terraform plan before activation. Do not run concurrent
Terraform writers outside the protected CI workflow. Secrets are passed by file,
not command-line value. See docs/AUTOMATION.md for prerequisites and recovery.
`;
function options(args) {
  const flags = new Set(["database-compatible", "maintenance-window"]);
  const values = new Set([
    "config",
    "target",
    "file",
    "identity",
    "name",
    "digest",
    "tag",
    "version",
    "execution",
    "snapshot",
    "new-id",
  ]);
  const result = {};
  for (let i = 0; i < args.length; i++) {
    const key = args[i].replace(/^--/, "");
    if (
      !args[i].startsWith("--") ||
      (!flags.has(key) && !values.has(key)) ||
      Object.hasOwn(result, key)
    )
      throw new Error("Unknown or duplicate option");
    if (flags.has(key)) result[key] = true;
    else {
      const value = args[++i];
      if (!value || value.startsWith("--"))
        throw new Error(`Missing --${key} value`);
      result[key] = value;
    }
  }
  return result;
}
const terraform = (stack, args, inherit = false) =>
  run(root, "terraform", [`-chdir=${resolve(root, "infra", stack)}`, ...args], {
    inherit,
    timeout: 2_700_000,
  });
function readTarget(c, o) {
  const t = o.target
    ? JSON.parse(readFileSync(resolve(root, o.target), "utf8"))
    : targetFromOutputs(JSON.parse(terraform("core", ["output", "-json"])));
  return validateTarget(c, t);
}
function refresh(ctx) {
  ctx.target = readTarget(ctx.config, {});
  atomicJson(resolve(root, "deployment-target.json"), ctx.target);
}
function checkExistingConfig() {
  for (const stack of ["bootstrap", "core"])
    if (existsSync(resolve(root, "infra", stack, "terraform.tfvars")))
      throw new Error(
        `Legacy infra/${stack}/terraform.tfvars exists. Merge it into deployment.json and remove it before using generated JSON configuration.`,
      );
}
async function exportTarget(c) {
  const t = readTarget(c, {});
  atomicJson(resolve(root, "deployment-target.json"), t);
  const bootstrap = JSON.parse(terraform("bootstrap", ["output", "-json"]));
  const backend = JSON.parse(
    terraform("bootstrap", ["output", "-json", "backend_config"]),
  );
  const vars = {
    DEPLOYMENT_ENVIRONMENT: c.environment,
    AWS_REGION: c.awsRegion,
    AWS_ACCOUNT_ID: c.awsAccountId,
    AWS_BUILD_ROLE_ARN: bootstrap.github_build_role_arn?.value,
    AWS_INFRASTRUCTURE_ROLE_ARN:
      bootstrap.github_infrastructure_role_arn?.value,
    TF_STATE_BUCKET: backend.bucket,
    TF_STATE_KEY: backend.key,
    TF_STATE_KMS_KEY: backend.kms_key_id,
  };
  atomicJson(resolve(root, "automation/local/github-variables.json"), vars);
  return t;
}
function initCore() {
  const backend = resolve(root, "infra/core/backend.local.hcl");
  if (!existsSync(backend))
    throw new Error(
      "Restore infra/core/backend.local.hcl from your private configuration or run up first.",
    );
  terraform(
    "core",
    [
      "init",
      "-input=false",
      "-lockfile=readonly",
      "-backend-config=backend.local.hcl",
    ],
    true,
  );
}
async function release(ctx, o, { rollback = false } = {}) {
  identityReady(ctx);
  assertUnlocked(ctx);
  const before = serviceState(ctx);
  const id = `release-${randomUUID()}`;
  const receiptPath = resolve(root, "evidence/runs", id, "release.json");
  const receipt = {
    id,
    recordedAt: new Date().toISOString(),
    status: "needs verification",
    previousTaskDefinition: before.taskDefinition,
    previousDesiredCount: before.desiredCount,
    steps: {},
  };
  atomicJson(receiptPath, receipt);
  try {
    const digest = o.digest ?? buildImage(ctx, o.tag ?? id);
    if (!/^sha256:[a-f0-9]{64}$/.test(digest))
      throw new Error("An immutable SHA-256 image digest is required");
    receipt.imageDigest = digest;
    receipt.steps.scan = await scanImage(ctx, digest);
    atomicJson(receiptPath, receipt);
    if (before.runningCount > 0 || before.desiredCount > 0) {
      receipt.steps.snapshot = await snapshotDatabase(ctx);
      atomicJson(receiptPath, receipt);
    }
    assertUnlocked(ctx);
    atomicJson(resolve(root, "infra/core/image.auto.tfvars.json"), {
      image_digest: digest,
    });
    terraform(
      "core",
      [
        "plan",
        "-input=false",
        "-lock-timeout=5m",
        "-var-file=image.auto.tfvars.json",
        "-out=release.tfplan",
      ],
      true,
    );
    // Interactive on a terminal; automation supplies a protected GitHub environment.
    if (!process.env.CI) {
      const { createInterface } = await import("node:readline/promises");
      const prompt = createInterface({
        input: process.stdin,
        output: process.stdout,
      });
      let answer;
      try {
        answer = await prompt.question(
          "Apply this saved plan and start the release? Type yes: ",
        );
      } finally {
        prompt.close();
      }
      if (answer !== "yes") throw new Error("Saved plan was not approved");
    }
    terraform(
      "core",
      ["apply", "-input=false", "-lock-timeout=5m", "release.tfplan"],
      true,
    );
    refresh(ctx);
    if (ctx.target.approved_image_digest !== digest)
      throw new Error("Applied image digest differs; no rollout started");
    receipt.taskDefinition = ctx.target.portal_task_definition_arn;
    receipt.workflowVersion = ctx.target.rollout_state_machine_version_arn;
    receipt.steps.rollout = await runRollout(ctx, id);
    receipt.steps.liveValidation = await validateLive(
      ctx,
      resolve(root, "evidence/runs", id, "validation.json"),
    );
    receipt.status = receipt.steps.liveValidation.status;
    receipt.kind = rollback ? "rollback" : "release";
    atomicJson(receiptPath, receipt);
    process.stdout.write(
      `Release recorded: ${receiptPath}\nStatus: ${receipt.status}\n`,
    );
    if (receipt.status === "blocked")
      throw new Error(
        "Deployment completed but live checks are blocked; inspect evidence. No automatic database downgrade attempted.",
      );
    return receipt;
  } catch (error) {
    receipt.status = "blocked";
    receipt.error = error.message;
    atomicJson(receiptPath, receipt);
    throw error;
  }
}
async function latestRelease() {
  const response = await fetch(
    "https://api.github.com/repos/backstage/backstage/releases/latest",
    {
      signal: AbortSignal.timeout(30_000),
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "backstage-aws-automation",
      },
    },
  );
  if (!response.ok)
    throw new Error(`GitHub release lookup failed: HTTP ${response.status}`);
  const value = await response.json();
  if (
    value.draft ||
    value.prerelease ||
    !/^v\d+\.\d+\.\d+$/.test(value.tag_name ?? "") ||
    !value.html_url?.startsWith(
      "https://github.com/backstage/backstage/releases/",
    )
  )
    throw new Error(
      "Release response is not a stable, canonical Backstage release",
    );
  return {
    version: value.tag_name.slice(1),
    url: value.html_url,
    publishedAt: value.published_at,
    checkedAt: new Date().toISOString(),
  };
}
async function main() {
  const [command, ...args] = process.argv.slice(2);
  if (!command || command === "help" || command === "--help") {
    process.stdout.write(help);
    return;
  }
  const o = options(args);
  if (command === "secrets-init") {
    writeIdentityTemplate(
      resolve(root, o.file ?? ".secrets/github-identity.json"),
    );
    console.log(
      "Created private identity template. Fill in the real OAuth values before uploading.",
    );
    return;
  }
  if (command === "latest") {
    const r = await latestRelease();
    atomicJson(resolve(root, "automation/local/latest-release.json"), r);
    console.log(JSON.stringify(r, null, 2));
    return;
  }
  if (command === "upgrade") {
    if (run(root, "git", ["status", "--porcelain"]).trim())
      throw new Error("Start the upgrade from a clean Git working tree");
    const r = o.version ? { version: o.version } : await latestRelease();
    if (!/^\d+\.\d+\.\d+$/.test(r.version))
      throw new Error("Invalid stable release version");
    run(
      root,
      "git",
      ["switch", "-c", `upgrade/backstage-${r.version}-${Date.now()}`],
      { inherit: true },
    );
    run(
      root,
      "node",
      [
        ".yarn/releases/yarn-4.13.0.cjs",
        "backstage-cli",
        "versions:bump",
        "--release",
        r.version,
      ],
      { cwd: resolve(root, "app"), inherit: true, timeout: 1_800_000 },
    );
    run(
      root,
      "node",
      [".yarn/releases/yarn-4.13.0.cjs", "install", "--immutable"],
      { cwd: resolve(root, "app"), inherit: true, timeout: 1_800_000 },
    );
    run(root, "node", [".yarn/releases/yarn-4.13.0.cjs", "tsc"], {
      cwd: resolve(root, "app"),
      inherit: true,
      timeout: 900_000,
    });
    run(
      root,
      "node",
      [
        ".yarn/releases/yarn-4.13.0.cjs",
        "test",
        "--watch=false",
        "--runInBand",
      ],
      { cwd: resolve(root, "app"), inherit: true, timeout: 900_000 },
    );
    console.log(
      "Upgrade branch prepared. Review migrations and plugin compatibility, run CI, then deploy the same image digest through staging and production.",
    );
    return;
  }
  const c = loadConfig(root, o.config);
  if (command === "configure") {
    checkExistingConfig();
    configure(root, c);
    console.log(
      "Generated nonsecret Terraform inputs and the trusted catalog owner.",
    );
    return;
  }
  if (command === "doctor") {
    for (const tool of ["terraform", "aws", "node", "docker", "jq"])
      run(root, tool, tool === "terraform" ? ["version"] : ["--version"]);
    const ctx = context(root, c, {});
    verifyIdentity(ctx);
    const summary = ctx.aws("iam", "get-account-summary").SummaryMap;
    if (summary?.AccountMFAEnabled !== 1)
      throw new Error(
        "Root MFA is not verified enabled; configure it before provisioning.",
      );
    console.log(
      "Tool/account/MFA preflight passed. DNS delegation and GitHub OAuth still require real configuration.",
    );
    return;
  }
  if (command === "up") {
    if (!o.identity)
      throw new Error(
        "up requires --identity FILE containing real GitHub OAuth credentials",
      );
    checkExistingConfig();
    configure(root, c);
    verifyIdentity(context(root, c, {}));
    const summary = context(root, c, {}).aws(
      "iam",
      "get-account-summary",
    ).SummaryMap;
    if (summary?.AccountMFAEnabled !== 1)
      throw new Error("Enable root MFA before provisioning");
    return withOperatorLock(root, async () => {
      run(root, "bash", ["scripts/provision.sh"], {
        inherit: true,
        timeout: 3_600_000,
      });
      const target = await exportTarget(c),
        ctx = context(root, c, target);
      setIdentity(ctx, resolve(root, o.identity));
      return release(ctx, o);
    });
  }
  if (command === "export-target") {
    await exportTarget(c);
    console.log(
      "Wrote deployment-target.json and local GitHub variables. No secret values exported.",
    );
    return;
  }
  if (command === "github-setup") {
    const variables = JSON.parse(
      readFileSync(
        resolve(root, "automation/local/github-variables.json"),
        "utf8",
      ),
    );
    for (const suffix of ["build", "infrastructure"]) {
      const environment = `${c.environment}-${suffix}`;
      // The owner must review deployments. The API fails if the plan/repository
      // cannot enforce this protection; never silently remove required reviewers.
      const body = JSON.stringify({
        wait_timer: 0,
        prevent_self_review: false,
        reviewers: [{ type: "User", id: Number(c.ownerGithubId) }],
        deployment_branch_policy: {
          protected_branches: false,
          custom_branch_policies: true,
        },
      });
      run(
        root,
        "gh",
        [
          "api",
          "--method",
          "PUT",
          `repos/${c.githubRepository}/environments/${environment}`,
          "--input",
          "-",
        ],
        { input: body },
      );
      const policies = JSON.parse(
        run(root, "gh", [
          "api",
          `repos/${c.githubRepository}/environments/${environment}/deployment-branch-policies`,
        ]),
      );
      if (
        !(policies.branch_policies ?? []).some(
          (x) => x.name === "main" && x.type === "branch",
        )
      )
        run(
          root,
          "gh",
          [
            "api",
            "--method",
            "POST",
            `repos/${c.githubRepository}/environments/${environment}/deployment-branch-policies`,
            "--input",
            "-",
          ],
          { input: JSON.stringify({ name: "main", type: "branch" }) },
        );
    }
    run(
      root,
      "gh",
      [
        "variable",
        "set",
        "DEPLOYMENT_TARGET_JSON",
        "--repo",
        c.githubRepository,
      ],
      { input: readFileSync(resolve(root, "deployment-target.json"), "utf8") },
    );
    for (const [key, value] of Object.entries(variables)) {
      if (typeof value !== "string" || !value)
        throw new Error(`Missing GitHub variable ${key}`);
      run(root, "gh", [
        "variable",
        "set",
        key,
        "--repo",
        c.githubRepository,
        "--body",
        value,
      ]);
    }
    run(
      root,
      "gh",
      [
        "variable",
        "set",
        "DEPLOYMENT_CONFIG_JSON",
        "--repo",
        c.githubRepository,
      ],
      { input: JSON.stringify(c) },
    );
    console.log(
      "Created reviewer-protected environments and repository variables. Enable branch protection, commit reviewed source, then dispatch Deploy.",
    );
    return;
  }
  if (!o.target) initCore();
  const ctx = context(root, c, readTarget(c, o));
  verifyIdentity(ctx);
  if (command === "build") {
    const digest = buildImage(ctx, o.tag ?? `release-${randomUUID()}`);
    console.log(digest);
    return;
  }
  if (command === "scan") {
    console.log(JSON.stringify(await scanImage(ctx, o.digest), null, 2));
    return;
  }
  if (command === "validate-live") {
    const result = await validateLive(ctx);
    console.log(JSON.stringify(result, null, 2));
    if (result.status === "blocked") process.exitCode = 1;
    return;
  }
  if (command === "validate-workflow") {
    const results = {};
    for (const [name, outputName] of [
      ["rollout", "rollout_definition"],
      ["database-rotation", "rotation_definition"],
    ]) {
      const output = JSON.parse(
        terraform("core", ["output", "-json", outputName]),
      );
      const machine = JSON.parse(output);
      jsonataOnly(machine);
      const path = resolve(root, "automation/local", name + ".asl.json");
      atomicJson(path, machine);
      results[name] = ctx.aws(
        "stepfunctions",
        "validate-state-machine-definition",
        [
          "--definition",
          "file://" + path,
          "--type",
          "STANDARD",
          "--severity",
          "WARNING",
        ],
      );
    }
    atomicJson(
      resolve(root, "automation/local/workflow-validation.json"),
      results,
    );
    if (Object.values(results).some((result) => result.result !== "OK"))
      throw new Error(
        "AWS rejected a workflow definition; inspect retained diagnostics",
      );
    console.log("Both JSONata-only contracts and AWS ASL validations passed.");
    return;
  }
  if (command === "recover-lock") {
    console.log(JSON.stringify(recoverLock(ctx, o.execution), null, 2));
    return;
  }
  return withOperatorLock(root, async () => {
    if (command === "release" || command === "rollback") {
      if (command === "rollback" && (!o.digest || !o["database-compatible"]))
        throw new Error(
          "Rollback requires --digest and --database-compatible after a real migration compatibility review",
        );
      return release(ctx, o, { rollback: command === "rollback" });
    }
    if (command === "snapshot") {
      assertUnlocked(ctx);
      console.log(JSON.stringify(await snapshotDatabase(ctx), null, 2));
      return;
    }
    if (command === "rollout") {
      identityReady(ctx);
      console.log(
        JSON.stringify(
          await runRollout(ctx, `rollout-${randomUUID()}`),
          null,
          2,
        ),
      );
      return;
    }
    if (command === "secrets-set" || command === "secrets-rotate") {
      assertUnlocked(ctx);
      if (!o.identity) throw new Error("Supply --identity FILE");
      const receipt = setIdentity(ctx, resolve(root, o.identity));
      if (command === "secrets-rotate") {
        identityReady(ctx);
        receipt.rollout = await runRollout(ctx, `identity-${randomUUID()}`);
        receipt.tasksReloaded = true;
      }
      atomicJson(
        resolve(root, "automation/local/identity-receipt.json"),
        receipt,
      );
      console.log(JSON.stringify(receipt, null, 2));
      return;
    }
    if (command === "secrets-put") {
      assertUnlocked(ctx);
      const definitions = c.additionalSecrets ?? {};
      const definition = definitions[o.name];
      if (!definition || !o.file)
        throw new Error("Supply a configured --name and --file");
      const arns = JSON.parse(
        terraform("core", ["output", "-json", "application_secret_arns"]),
      );
      const secretArn = arns[o.name];
      if (!secretArn)
        throw new Error(
          "Apply the additional secret definition before uploading",
        );
      const file = resolve(root, o.file);
      chmodSync(file, 0o600);
      const text = readFileSync(file, "utf8");
      if (Buffer.byteLength(text) < 1 || Buffer.byteLength(text) > 65536)
        throw new Error("Secret must contain 1–65536 bytes");
      if (definition.jsonKey) {
        let value;
        try { value = JSON.parse(text); }
        catch { throw new Error('Plugin secret file is not valid JSON; no content is logged'); }
        if (typeof value?.[definition.jsonKey] !== 'string') throw new Error('Configured JSON key is absent or not a string');
      }
      const result = ctx.aws("secretsmanager", "put-secret-value", [
        "--secret-id",
        secretArn,
        "--secret-string",
        `file://${file}`,
      ]);
      console.log(
        JSON.stringify({
          arn: result.ARN,
          versionId: result.VersionId,
          tasksReloaded: false,
        }),
      );
      return;
    }
    if (command === "db-rotate") {
      if (!o["maintenance-window"])
        throw new Error(
          "Database rotation requires --maintenance-window: this intentionally drains Backstage to zero tasks",
        );
      assertUnlocked(ctx);
      identityReady(ctx);
      await snapshotDatabase(ctx);
      // Operation is chosen by a validated enum; task ARN and all credentials are fixed in Terraform.
      const id = `db-rotate-${randomUUID()}`;
      const result = await runRollout(ctx, id, "rotateDatabase");
      const validation = await validateLive(ctx);
      atomicJson(resolve(root, "automation/local/database-rotation.json"), {
        ...result,
        status: validation.status,
      });
      console.log(
        "Rotation finished; live verification status: " + validation.status,
      );
      if (validation.status === "blocked") process.exitCode = 1;
      return;
    }
    if (command === "restore-drill") {
      if (
        !/^[a-z][a-z0-9-]{0,62}$/.test(o["new-id"] ?? "") ||
        o["new-id"] === ctx.target.database_identifier ||
        !o.snapshot
      )
        throw new Error(
          "Supply an isolated --new-id and a --snapshot ID; the active database is never overwritten",
        );
      const db = ctx.aws("rds", "describe-db-instances", [
        "--db-instance-identifier",
        ctx.target.database_identifier,
      ]).DBInstances?.[0];
      if (!db) throw new Error("Source database is unknown");
      const result = ctx.aws("rds", "restore-db-instance-from-db-snapshot", [
        "--db-instance-identifier",
        o["new-id"],
        "--db-snapshot-identifier",
        o.snapshot,
        "--db-instance-class",
        db.DBInstanceClass,
        "--db-subnet-group-name",
        db.DBSubnetGroup.DBSubnetGroupName,
        "--vpc-security-group-ids",
        ...db.VpcSecurityGroups.map((s) => s.VpcSecurityGroupId),
        "--db-parameter-group-name",
        db.DBParameterGroups[0].DBParameterGroupName,
        "--no-publicly-accessible",
        "--no-multi-az",
        "--deletion-protection",
        "--tags",
        `Key=Project,Value=${c.name}`,
        "Key=Purpose,Value=RecoveryDrill",
      ]);
      atomicJson(resolve(root, "automation/local/restore-drill.json"), {
        databaseId: o["new-id"],
        snapshotId: o.snapshot,
        status: "needs verification",
        databaseArn: result.DBInstance?.DBInstanceArn,
        cutover: false,
      });
      console.log(
        "Isolated restore started. No traffic cutover performed. See docs/RECOVERY.md to validate credentials, data and Terraform adoption.",
      );
      return;
    }
    throw new Error(`Unknown command: ${command}`);
  });
}
main().catch((error) => {
  process.stderr.write(`Blocked: ${error.message}\n`);
  process.exitCode = 1;
});
