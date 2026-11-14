import { readFileSync, writeFileSync, mkdirSync, renameSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { randomUUID } from "node:crypto";

export function atomicJson(path, value) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${randomUUID()}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, {
    mode: 0o600,
    flag: "wx",
  });
  renameSync(temporary, path);
}
export function validateConfig(c) {
  const require = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  require(c &&
    typeof c === "object" &&
    !Array.isArray(c), "Expected one configuration object");
  const allowed = new Set(['awsAccountId','awsRegion','name','environment','domainName','route53ZoneId','allowedIngressCidrs','githubRepository','ownerGithubId','budgetEmail','monthlyBudgetUsd','alertEmail','accountBaseline','singleNatGateway','databaseMultiAz','portalDesiredCount','maximumHighFindings','maximumCriticalFindings','additionalSecrets']);
  require(Object.keys(c).every(key => allowed.has(key)), 'Unknown configuration field. Secret values belong in private files, not deployment.json.');
  for (const [key, pattern] of Object.entries({
    awsAccountId: /^\d{12}$/,
    awsRegion: /^[a-z]{2}-[a-z]+-\d+$/,
    name: /^[a-z][a-z0-9-]{2,19}$/,
    environment: /^[a-z][a-z0-9-]{1,19}$/,
    domainName: /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?\.[a-z]{2,}$/,
    route53ZoneId: /^Z[A-Z0-9]{5,32}$/,
    githubRepository: /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/,
    ownerGithubId: /^[1-9][0-9]{0,19}$/,
  }))
    require(typeof c[key] === "string" &&
      pattern.test(c[key]), `Invalid ${key}`);
  require(Number.isSafeInteger(
    Number(c.ownerGithubId),
  ), "GitHub numeric ID must be a safe integer");
  require(Array.isArray(c.allowedIngressCidrs) &&
    c.allowedIngressCidrs.length > 0 &&
    c.allowedIngressCidrs.length <=
      50, "Supply 1–50 explicit IPv4 ingress ranges");
  for (const cidr of c.allowedIngressCidrs) {
    const match =
      typeof cidr === "string"
        ? cidr.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\/(\d{1,2})$/)
        : null;
    require(match &&
      match.slice(1, 5).every((x) => Number(x) <= 255) &&
      Number(match[5]) >= 8 &&
      Number(match[5]) <=
        32, "Ingress must be IPv4 CIDRs /8–/32; world-open ranges are rejected");
  }
  for (const key of ["budgetEmail", "alertEmail"])
    require(typeof c[key] === "string" &&
      /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(c[key]), `Invalid ${key}`);
  require(Number.isFinite(c.monthlyBudgetUsd) &&
    c.monthlyBudgetUsd >= 1 &&
    c.monthlyBudgetUsd <= 100000, "Invalid monthlyBudgetUsd");
  for (const key of ["accountBaseline", "singleNatGateway", "databaseMultiAz"])
    require(typeof c[key] === "boolean", `Expected Boolean ${key}`);
  require(Number.isInteger(c.portalDesiredCount) &&
    c.portalDesiredCount >= 1 &&
    c.portalDesiredCount <= 10, "portalDesiredCount must be 1–10");
  for (const key of ["maximumHighFindings", "maximumCriticalFindings"])
    require(Number.isInteger(c[key]) && c[key] >= 0, `Invalid ${key}`);
  if (c.additionalSecrets !== undefined) {
    require(c.additionalSecrets &&
      typeof c.additionalSecrets === "object" &&
      !Array.isArray(c.additionalSecrets), "Invalid additionalSecrets");
    const names = new Set();
    for (const [name, secret] of Object.entries(c.additionalSecrets)) {
      require(/^[a-z][a-z0-9_-]{0,40}$/.test(name) &&
        secret &&
        typeof secret === "object", "Invalid additional secret name");
      require(/^[A-Z][A-Z0-9_]{0,80}$/.test(secret.environmentVariable) &&
        !/^(AWS_|POSTGRES_|AUTH_|NODE_|APP_|BACKEND_|PGSSL)/.test(
          secret.environmentVariable,
        ) &&
        !names.has(
          secret.environmentVariable,
        ), "Secret environment names must be unique and nonreserved");
      require(secret.jsonKey == null ||
        /^[A-Za-z0-9_-]{1,80}$/.test(
          secret.jsonKey,
        ), "Invalid secret JSON key");
      require(Object.keys(secret).every(key => ['environmentVariable','jsonKey','description'].includes(key)), 'Secret descriptors must contain references only, never credential values');
      require(secret.description === undefined || (typeof secret.description === 'string' && secret.description.length <= 300), 'Invalid secret description');
      names.add(secret.environmentVariable);
    }
  }
  return c;
}
export function loadConfig(root, path = "deployment.json") {
  let value;
  try { value = JSON.parse(readFileSync(resolve(root, path), 'utf8')); }
  catch { throw new Error('Configuration file is missing or is not valid JSON'); }
  return validateConfig(value);
}
export function configure(root, c) {
  validateConfig(c);
  atomicJson(resolve(root, "infra/bootstrap/terraform.tfvars.json"), {
    aws_account_id: c.awsAccountId,
    aws_region: c.awsRegion,
    name: c.name,
    enable_account_baseline: c.accountBaseline,
    github_repository: c.githubRepository,
    github_environment: `${c.environment}-infrastructure`,
    github_build_environment: `${c.environment}-build`,
    budget_email: c.budgetEmail,
    monthly_budget_usd: c.monthlyBudgetUsd,
  });
  atomicJson(resolve(root, "infra/core/terraform.tfvars.json"), {
    aws_account_id: c.awsAccountId,
    aws_region: c.awsRegion,
    name: c.name,
    environment: c.environment,
    domain_name: c.domainName,
    route53_zone_id: c.route53ZoneId,
    allowed_ingress_cidrs: c.allowedIngressCidrs,
    single_nat_gateway: c.singleNatGateway,
    database_multi_az: c.databaseMultiAz,
    portal_desired_count: c.portalDesiredCount,
    alert_email: c.alertEmail,
    application_secrets: Object.fromEntries(
      Object.entries(c.additionalSecrets ?? {}).map(([key, v]) => [
        key,
        {
          environment_variable: v.environmentVariable,
          json_key: v.jsonKey ?? null,
          description: v.description ?? "Backstage plugin credential",
        },
      ]),
    ),
  });
  const org = resolve(root, "app/catalog/org.yaml");
  let text = readFileSync(org, "utf8");
  text = text.replace(
    /github\.com\/user-id:\s*['"][^'"\n]+['"]/,
    `github.com/user-id: '${c.ownerGithubId}'`,
  );
  writeFileSync(org, text);
}
