import { atomicJson } from "./config.mjs";
import { resolve } from "node:path";
import { serviceState } from "./runtime.mjs";

export function jsonataOnly(machine) {
  if (machine.QueryLanguage !== "JSONata")
    throw new Error("Top-level QueryLanguage must be JSONata");
  function walk(value) {
    if (!value || typeof value !== "object") return;
    for (const [key, child] of Object.entries(value)) {
      if (
        [
          "InputPath",
          "OutputPath",
          "ResultPath",
          "ResultSelector",
          "Parameters",
        ].includes(key) ||
        key.endsWith(".$")
      )
        throw new Error(`JSONPath field rejected: ${key}`);
      if (key === "QueryLanguage" && child !== "JSONata")
        throw new Error("JSONPath state rejected");
      walk(child);
    }
  }
  walk(machine);
  return true;
}
export function classifyRelease(checks) {
  if (checks.some((c) => c.status === "blocked")) return "blocked";
  if (checks.some((c) => c.status !== "complete")) return "needs verification";
  return "complete";
}
export async function validateLive(ctx, outputFile, fetcher = fetch) {
  const checks = [];
  const record = (name, passed, detail) =>
    checks.push({ name, status: passed ? "complete" : "blocked", detail });
  const inspect = (name, fn) => {
    try {
      const [passed, detail] = fn();
      record(name, passed, detail);
    } catch {
      checks.push({
        name,
        status: "blocked",
        detail:
          "AWS inspection failed. Check credentials and permissions; no success assumed.",
      });
    }
  };
  inspect("ECS exact revision and healthy counts", () => {
    const s = serviceState(ctx);
    return [
      s.taskDefinition === ctx.target.portal_task_definition_arn &&
        s.runningCount === ctx.config.portalDesiredCount &&
        s.pendingCount === 0 &&
        s.deployments?.length === 1 &&
        s.deployments[0].rolloutState === "COMPLETED",
      {
        taskDefinition: s.taskDefinition,
        running: s.runningCount,
        desired: s.desiredCount,
        pending: s.pendingCount,
      },
    ];
  });
  inspect("ALB target health", () => {
    const result =
      ctx.aws("elbv2", "describe-target-health", [
        "--target-group-arn",
        ctx.target.target_group_arn,
      ]).TargetHealthDescriptions ?? [];
    return [
      result.length === ctx.config.portalDesiredCount &&
        result.every((t) => t.TargetHealth?.State === "healthy"),
      result.map((t) => ({ id: t.Target?.Id, state: t.TargetHealth?.State })),
    ];
  });
  inspect("RDS privacy, encryption and backups", () => {
    const d = ctx.aws("rds", "describe-db-instances", [
      "--db-instance-identifier",
      ctx.target.database_identifier,
    ]).DBInstances?.[0];
    return [
      d?.PubliclyAccessible === false &&
        d?.StorageEncrypted === true &&
        d?.BackupRetentionPeriod >= 7 &&
        d?.DeletionProtection === true &&
        d?.DBInstanceStatus === "available",
      {
        publiclyAccessible: d?.PubliclyAccessible,
        encrypted: d?.StorageEncrypted,
        backupRetentionDays: d?.BackupRetentionPeriod,
        deletionProtection: d?.DeletionProtection,
        status: d?.DBInstanceStatus,
      },
    ];
  });
  inspect("SNS email confirmation", () => {
    const subs =
      ctx.aws("sns", "list-subscriptions-by-topic", [
        "--topic-arn",
        ctx.target.operations_topic_arn,
      ]).Subscriptions ?? [];
    return [
      subs.some(
        (s) =>
          s.Protocol === "email" &&
          s.Endpoint === ctx.config.alertEmail &&
          s.SubscriptionArn !== "PendingConfirmation",
      ),
      "Email subscription must be confirmed by its recipient",
    ];
  });
  const http = async (name, path, expected, token) => {
    try {
      const response = await fetcher(`${ctx.target.portal_url}${path}`, {
        signal: AbortSignal.timeout(15_000),
        redirect: "manual",
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      });
      record(name, expected.includes(response.status), {
        status: response.status,
      });
      await response.body?.cancel();
    } catch {
      checks.push({
        name,
        status: "blocked",
        detail:
          "HTTPS check failed. Run from an allowed network; certificate validation is never disabled.",
      });
    }
  };
  await http(
    "Backstage readiness over HTTPS",
    "/.backstage/health/v1/readiness",
    [200],
  );
  await http("Frontend loads over HTTPS", "/", [200]);
  await http(
    "Anonymous catalog access denied",
    "/api/catalog/entities",
    [401, 403],
  );
  if (process.env.BACKSTAGE_SMOKE_TOKEN)
    await http(
      "Authenticated catalog smoke test",
      "/api/catalog/entities?limit=1",
      [200],
      process.env.BACKSTAGE_SMOKE_TOKEN,
    );
  else
    checks.push({
      name: "Authenticated catalog smoke test",
      status: "needs verification",
      detail:
        "Supply a valid BACKSTAGE_SMOKE_TOKEN in the process environment; it is never recorded in evidence.",
    });
  checks.push({
    name: "Interactive GitHub login and operator permissions",
    status: "needs verification",
    detail:
      "A human must validate real sign-in and operator/nonoperator behavior; HTTP health is insufficient.",
  });
  checks.push({
    name: "Restore rehearsal and upgrade schema compatibility",
    status: "needs verification",
    detail:
      "Validate the restored database with this release and retain the measured recovery result.",
  });
  const result = {
    recordedAt: new Date().toISOString(),
    status: classifyRelease(checks),
    checks,
    scope: "Selected live checks; not a comprehensive security attestation",
  };
  atomicJson(
    outputFile ?? resolve(ctx.root, "automation/local/live-validation.json"),
    result,
  );
  return result;
}
