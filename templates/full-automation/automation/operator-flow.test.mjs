import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
  readdirSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
const config = {
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
const arn = (s, r) => `arn:aws:${s}:us-east-1:123456789012:${r}`;
const target = {
  aws_account_id: config.awsAccountId,
  aws_region: config.awsRegion,
  portal_url: "https://backstage.example.com",
  ecs_service_name: "backstage-portal",
  ecr_repository_name: "backstage/portal",
  ecr_repository_url:
    "123456789012.dkr.ecr.us-east-1.amazonaws.com/backstage/portal",
  release_lock_table: "backstage-release-lock",
  identity_secret_arn: arn("secretsmanager", "secret:identity"),
  database_application_secret_arn: arn("secretsmanager", "secret:db"),
  rollout_state_machine_arn: arn(
    "states",
    "stateMachine:backstage-portal-rollout",
  ),
  ecs_cluster_arn: arn("ecs", "cluster/backstage-cluster"),
  portal_task_definition_arn: arn("ecs", "task-definition/backstage-portal:7"),
  database_identifier: "backstage-postgres",
  approved_image_digest: "sha256:" + "b".repeat(64),
};
function fixture(t, mode) {
  const root = mkdtempSync(resolve(tmpdir(), "backstage-flow-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const dir of ["automation", "infra/core", "bin"])
    mkdirSync(resolve(root, dir), { recursive: true });
  for (const file of ["cli.mjs", "runtime.mjs", "config.mjs", "validation.mjs"])
    copyFileSync(
      new URL(file, import.meta.url),
      resolve(root, "automation", file),
    );
  writeFileSync(resolve(root, "deployment.json"), JSON.stringify(config));
  writeFileSync(resolve(root, "target.json"), JSON.stringify(target));
  writeFileSync(
    resolve(root, "infra/core/backend.local.hcl"),
    "# fake test backend\n",
  );
  const executable = `#!/usr/bin/env node
const fs=require('node:fs'),path=require('node:path'),args=process.argv.slice(2),root=process.env.FLOW_ROOT,tool=path.basename(process.argv[1]),target=JSON.parse(fs.readFileSync(path.join(root,'target.json'))),mode=process.env.FLOW_MODE;
fs.appendFileSync(path.join(root,'calls.jsonl'),JSON.stringify({tool,args})+'\\n');
let value={};
if(tool==='terraform'){
 if(args[1]==='output'){if(fs.existsSync(path.join(root,'applied'))) {target.portal_task_definition_arn=target.portal_task_definition_arn.replace(':7',':8');target.approved_image_digest='sha256:'+'a'.repeat(64);}value=Object.fromEntries(Object.entries(target).map(([k,v])=>[k,{value:v}]));}
 if(args[1]==='apply')fs.writeFileSync(path.join(root,'applied'),'yes');
}else{
 const operation=args[3];
 if(operation==='get-caller-identity')value={Account:'123456789012',Arn:'arn:aws:iam::123456789012:role/operator'};
 if(operation==='describe-secret')value=mode==='missing-secret'?{}:{VersionIdsToStages:{version:['AWSCURRENT']}};
 if(operation==='get-item')value=mode==='locked'?{Item:{Owner:{S:'another-execution'}}}:{};
 if(operation==='describe-services')value={services:[{status:'ACTIVE',desiredCount:2,runningCount:2,pendingCount:0,taskDefinition:target.portal_task_definition_arn,deployments:[{rolloutState:'COMPLETED'}]}]};
 if(operation==='describe-image-scan-findings')value={imageScanStatus:{status:'COMPLETE'},imageScanFindings:{imageScanCompletedAt:new Date().toISOString(),findingSeverityCounts:{HIGH:mode==='scan-blocked'?1:0}}};
 if(operation==='describe-db-snapshots')value={DBSnapshots:[{Status:'available',DBSnapshotArn:'arn:snapshot:fixture'}]};
 if(operation==='start-execution')value={executionArn:'arn:aws:states:us-east-1:123456789012:execution:backstage-portal-rollout:fixture'};
 if(operation==='describe-execution')value={status:'FAILED'};
}
process.stdout.write(JSON.stringify(value));
`;
  for (const tool of ["aws", "terraform"])
    writeFileSync(resolve(root, "bin", tool), executable, { mode: 0o755 });
  const result = spawnSync(
    process.execPath,
    ["automation/cli.mjs", "release", "--digest", "sha256:" + "a".repeat(64)],
    {
      cwd: root,
      encoding: "utf8",
      timeout: 20_000,
      env: {
        ...process.env,
        PATH: resolve(root, "bin") + ":" + process.env.PATH,
        CI: "true",
        FLOW_ROOT: root,
        FLOW_MODE: mode,
      },
    },
  );
  const calls = readFileSync(resolve(root, "calls.jsonl"), "utf8")
    .trim()
    .split("\n")
    .map(JSON.parse);
  return { root, result, calls };
}
test("real operator CLI stops before snapshot/apply/rollout when a secret, lock or image gate blocks", (t) => {
  for (const mode of ["missing-secret", "locked", "scan-blocked"]) {
    const { result, calls } = fixture(t, mode);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Blocked:/);
    assert.ok(
      !calls.some((c) => c.tool === "terraform" && c.args[1] === "apply"),
    );
    assert.ok(
      !calls.some(
        (c) =>
          c.args[3] === "create-db-snapshot" || c.args[3] === "start-execution",
      ),
    );
  }
});
test("real operator CLI snapshots before applying a pinned image; failed remote rollout is retained as blocked evidence", (t) => {
  const { root, result, calls } = fixture(t, "failed-rollout");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Execution FAILED/);
  const at = (predicate) => calls.findIndex(predicate);
  const snapshot = at((c) => c.args[3] === "create-db-snapshot"),
    apply = at((c) => c.tool === "terraform" && c.args[1] === "apply"),
    start = at((c) => c.args[3] === "start-execution");
  assert.ok(snapshot >= 0 && apply > snapshot && start > apply);
  const directory = resolve(
    root,
    "evidence/runs",
    readdirSync(resolve(root, "evidence/runs"))[0],
  );
  const receipt = JSON.parse(
    readFileSync(resolve(directory, "release.json"), "utf8"),
  );
  assert.equal(receipt.status, "blocked");
  assert.equal(
    receipt.previousTaskDefinition,
    target.portal_task_definition_arn,
  );
  assert.equal(receipt.imageDigest, "sha256:" + "a".repeat(64));
  assert.equal(receipt.steps.snapshot.status, "complete");
  assert.equal(receipt.steps.rollout, undefined);
  const image = JSON.parse(
    readFileSync(resolve(root, "infra/core/image.auto.tfvars.json"), "utf8"),
  );
  assert.equal(image.image_digest, receipt.imageDigest);
});
