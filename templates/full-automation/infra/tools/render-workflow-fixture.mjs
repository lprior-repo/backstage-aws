#!/usr/bin/env node
/**
 * Render the real workflow expression with deterministic fixture resource IDs.
 * This invokes provider-free `terraform console`; it neither loads the AWS
 * provider nor configures AWS credentials, networking, or cloud resources.
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const infraDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = readFileSync(join(infraDirectory, 'core', 'workflow.tf'), 'utf8');
const match = source.match(/\bdefinition\s*=\s*([\s\S]*?)\n\s*depends_on\s*=/);
if (!match) throw new Error('Cannot find the workflow definition expression.');

const maintenance = readFileSync(join(infraDirectory, 'core', 'maintenance.tf'), 'utf8');
const maintenanceMatch = maintenance.match(/\bdefinition\s*=\s*([\s\S]*?)\n\}\noutput/);
if (!maintenanceMatch) throw new Error('Cannot find maintenance definition expression');
const maintenanceExpression = maintenanceMatch[1].trim().replaceAll('aws_sfn_state_machine.rollout.definition', 'local.definition');
const replacements = new Map([
  ['aws_dynamodb_table.release_lock.name', 'local.lock_table'],
  ['aws_ecs_cluster.portal.arn', 'local.cluster_arn'],
  ['aws_ecs_task_definition.rotate.arn', 'local.rotate_task_arn'],
  ['aws_ecs_task_definition.bootstrap.arn', 'local.bootstrap_task_arn'],
  ['aws_ecs_task_definition.portal.arn', 'local.portal_task_arn'],
  ['aws_subnet.application[*].id', 'local.application_subnets'],
  ['aws_security_group.bootstrap.id', 'local.bootstrap_security_group'],
  ['aws_ecs_service.portal.name', 'local.service_name'],
  ['aws_ecs_service.portal.id', 'local.service_arn'],
]);
let expression = match[1].trim();
for (const [reference, fixture] of replacements) {
  expression = expression.replaceAll(reference, fixture);
}
if (/\baws_[a-z_]+\./u.test(expression)) {
  throw new Error('An unhandled AWS resource reference remains in the expression.');
}

const fixtureDirectory = join(infraDirectory, 'core', 'tests', 'fixtures');
mkdirSync(fixtureDirectory, { recursive: true });
const terraformBinary = process.env.TERRAFORM_BIN || 'terraform';
for (const published of [false, true]) {
  const temporaryDirectory = mkdtempSync(join(infraDirectory, '.fixture-'));
  try {
    const configuration = `
terraform { required_version = "= 1.17.2" }
variable "image_digest" {
  type = string
  default = ${published ? '"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"' : 'null'}
}
variable "portal_desired_count" {
  type = number
  default = 2
}
locals {
  lock_table = "backstage-release-lock"
  partition = "aws"
  cluster_arn = "arn:aws:ecs:us-east-1:123456789012:cluster/backstage-cluster"
  rotate_task_arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-db-rotate:3"
  bootstrap_task_arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-db-bootstrap:3"
  portal_task_arn = "arn:aws:ecs:us-east-1:123456789012:task-definition/backstage-portal:7"
  application_subnets = ["subnet-11111111", "subnet-22222222"]
  bootstrap_security_group = "sg-11111111"
  service_name = "backstage-portal"
  service_arn = "arn:aws:ecs:us-east-1:123456789012:service/backstage-cluster/backstage-portal"
  portal_base_url = "https://backstage.example.com"
  definition = ${expression}
  rotation_definition = ${maintenanceExpression}
}
`;
    writeFileSync(join(temporaryDirectory, 'main.tf'), configuration, { mode: 0o600 });
    const output = execFileSync(terraformBinary, ['console', '-no-color'], {
      cwd: temporaryDirectory,
      input: 'local.definition\n',
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
      timeout: 60_000,
      env: { ...process.env, CHECKPOINT_DISABLE: '1', TF_IN_AUTOMATION: '1' },
    });
    const definition = JSON.parse(JSON.parse(output.trim()));
    definition.Comment = `TEST FIXTURE: synthetic account, ARN, and network identifiers; do not deploy. ${definition.Comment}`;
    const filename = `portal-rollout-${published ? 'published' : 'infrastructure-only'}.asl.json`;
    writeFileSync(join(fixtureDirectory, filename), `${JSON.stringify(definition, null, 2)}\n`);
    process.stdout.write(`Rendered ${filename} from workflow.tf\n`);
    const rotationOutput = execFileSync(terraformBinary, ['console', '-no-color'], {
      cwd: temporaryDirectory, input: 'local.rotation_definition\n', encoding: 'utf8',
      maxBuffer: 1024 * 1024, timeout: 60_000,
      env: { ...process.env, CHECKPOINT_DISABLE: '1', TF_IN_AUTOMATION: '1' },
    });
    const rotation = JSON.parse(JSON.parse(rotationOutput.trim()));
    rotation.Comment = 'TEST FIXTURE: synthetic identifiers; do not deploy. ' + rotation.Comment;
    writeFileSync(join(fixtureDirectory, 'database-rotation-' + (published ? 'published' : 'infrastructure-only') + '.asl.json'), JSON.stringify(rotation, null, 2) + '\n');
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}
