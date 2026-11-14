#!/usr/bin/env node
// A fixed, idempotent ECS job. It never accepts SQL, a host, or credentials from
// Step Functions input. Configuration is baked into the Terraform task definition.
import { createHash, createHmac, pbkdf2Sync, randomBytes } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export class BootstrapError extends Error {
  constructor(code) {
    super(code);
    this.name = 'BootstrapError';
    this.code = code;
  }
}

function required(env, name, maximum = 512) {
  const value = env[name];
  if (typeof value !== 'string' || !value || value.length > maximum || value.includes('\0')) {
    throw new BootstrapError(`INVALID_${name}`);
  }
  return value;
}

export function readConfiguration(env) {
  const config = {
    rotatePassword: env.DB_ROTATE_PASSWORD === 'true',
    region: required(env, 'AWS_REGION', 32),
    host: required(env, 'POSTGRES_HOST', 253),
    port: Number(required(env, 'POSTGRES_PORT', 5)),
    database: required(env, 'POSTGRES_DB', 63),
    appUser: required(env, 'DB_APP_USER', 63),
    adminSecretArn: required(env, 'DB_ADMIN_SECRET_ARN'),
    appSecretArn: required(env, 'DB_APP_SECRET_ARN'),
    caPath: required(env, 'PGSSLROOTCERT', 256),
  };
  if (!/^[a-z][a-z0-9_]{0,62}$/.test(config.database) || !/^[a-z][a-z0-9_]{0,62}$/.test(config.appUser)) {
    throw new BootstrapError('INVALID_DATABASE_IDENTIFIER');
  }
  if (!/^[a-z0-9-]+$/.test(config.region) || !/^[a-zA-Z0-9.-]+$/.test(config.host) || !Number.isInteger(config.port) || config.port < 1 || config.port > 65535) {
    throw new BootstrapError('INVALID_CONNECTION_SETTINGS');
  }
  if (config.adminSecretArn === config.appSecretArn || !config.adminSecretArn.includes(':secretsmanager:') || !config.appSecretArn.includes(':secretsmanager:')) {
    throw new BootstrapError('INVALID_SECRET_REFERENCES');
  }
  return Object.freeze(config);
}

export function parseCredentials(value, expectedUser) {
  let parsed;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new BootstrapError('INVALID_SECRET_JSON');
  }
  if (!parsed || typeof parsed.username !== 'string' || !parsed.username || parsed.username.length > 63 || typeof parsed.password !== 'string' || parsed.password.length < 16 || parsed.password.length > 256 || parsed.password.includes('\0')) {
    throw new BootstrapError('INVALID_SECRET_CREDENTIALS');
  }
  if (expectedUser && (parsed.username !== expectedUser || !/^[A-Za-z0-9_-]{32,128}$/.test(parsed.password))) {
    throw new BootstrapError('APP_SECRET_USER_OR_PASSWORD_INVALID');
  }
  return { username: parsed.username, password: parsed.password };
}

export function scramVerifier(password, salt = randomBytes(16)) {
  // ASCII base64url passwords avoid SASLprep/Unicode normalization ambiguity.
  // Production credentials enforce 32 characters minimum in parseCredentials;
  // this primitive also accepts the shorter published RFC 7677 test password.
  if (typeof password !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(password) || !Buffer.isBuffer(salt) || salt.length !== 16) {
    throw new BootstrapError('INVALID_SCRAM_INPUT');
  }
  // RFC 5802 section 3 / RFC 7677: PBKDF2-HMAC-SHA-256, ClientKey,
  // StoredKey and ServerKey. All cryptographic primitives come from Node.
  // PostgreSQL 16 defaults to 4096 iterations and accepts this verifier format:
  // https://www.postgresql.org/docs/16/runtime-config-connection.html
  // https://www.postgresql.org/docs/16/sql-createrole.html
  const iterations = 4096;
  const saltedPassword = pbkdf2Sync(password, salt, iterations, 32, 'sha256');
  const clientKey = createHmac('sha256', saltedPassword).update('Client Key').digest();
  const storedKey = createHash('sha256').update(clientKey).digest('base64');
  const serverKey = createHmac('sha256', saltedPassword).update('Server Key').digest('base64');
  return `SCRAM-SHA-256$${iterations}:${salt.toString('base64')}$${storedKey}:${serverKey}`;
}

function quoteIdentifier(value) {
  // User and database identifiers have already passed the strict ASCII check.
  return `"${value.replaceAll('"', '""')}"`;
}

function quoteLiteral(value) {
  return `'${value.replaceAll("'", "''")}'`;
}

export async function bootstrapDatabase(config, io) {
  const admin = parseCredentials(await io.readAdminSecret());
  if (admin.username === config.appUser) throw new BootstrapError('ADMIN_AND_APP_USER_MUST_DIFFER');
  const client = await io.connect(admin);
  try {
    const lock = await client.query('SELECT pg_try_advisory_lock(19700105, 7007) AS locked');
    if (lock.rows?.[0]?.locked !== true) throw new BootstrapError('BOOTSTRAP_ALREADY_RUNNING');
    const existing = await io.readAppSecret();
    if (config.rotatePassword === true) await inspectApplicationRole(client, config.appUser);
    const credentials = existing === null || config.rotatePassword === true
      ? { username: config.appUser, password: io.randomPassword() }
      : parseCredentials(existing, config.appUser);
    // Persist first, while holding the DB session lock. A crash is recoverable:
    // the next run uses this same password and reconciles the database role.
    if (existing === null || config.rotatePassword === true) {
      parseCredentials(JSON.stringify(credentials), config.appUser);
      await io.writeAppSecret(JSON.stringify(credentials));
    }
    await reconcileRole(client, config, credentials);
    io.log('Database application account is ready.');
  } finally {
    // Closing the session releases the advisory lock even on partial failure.
    await client.end();
  }
}

async function reconcileRole(client, config, credentials) {
  const role = quoteIdentifier(config.appUser);
  const database = quoteIdentifier(config.database);
  if (!(await inspectApplicationRole(client, config.appUser))) {
    // CREATE ROLE defaults to NOLOGIN and all privileged attributes false.
    // RDS master users have CREATEROLE but are not PostgreSQL superusers.
    await client.query(`CREATE ROLE ${role}`);
    if (!(await inspectApplicationRole(client, config.appUser))) {
      throw new BootstrapError('APPLICATION_ROLE_NOT_CREATED');
    }
  }
  // Send a verifier, never the plaintext password, in ALTER ROLE. PostgreSQL
  // warns that SQL can appear in server logs or activity displays:
  // https://www.postgresql.org/docs/16/libpq-misc.html#LIBPQ-PQENCRYPTPASSWORDCONN
  // The verifier is still sensitive (including to offline password guessing),
  // so restrict database/log access and do not log this statement ourselves.
  await client.query('SET standard_conforming_strings = on');
  // Even specifying NOSUPERUSER/NOREPLICATION/NOBYPASSRLS in ALTER ROLE can
  // require attributes the RDS master lacks. Inspect and reject elevated roles
  // instead of attempting to reset their privileged flags or memberships.
  // https://www.postgresql.org/docs/16/sql-alterrole.html
  await client.query(`ALTER ROLE ${role} WITH LOGIN PASSWORD ${quoteLiteral(scramVerifier(credentials.password))}`);
  await client.query(`REVOKE ALL ON DATABASE ${database} FROM PUBLIC`);
  await client.query(`GRANT CONNECT, CREATE, TEMPORARY ON DATABASE ${database} TO ${role}`);
  await client.query('REVOKE CREATE ON SCHEMA public FROM PUBLIC');
  await client.query(`GRANT USAGE ON SCHEMA public TO ${role}`);
}

async function inspectApplicationRole(client, appUser) {
  const result = await client.query(
    'SELECT r.rolsuper, r.rolcreatedb, r.rolcreaterole, r.rolreplication, r.rolbypassrls, EXISTS (SELECT 1 FROM pg_auth_members m WHERE m.member = r.oid) AS has_memberships FROM pg_roles r WHERE r.rolname = $1',
    [appUser],
  );
  if (result.rowCount === 0) return false;
  const role = result.rows?.[0];
  if (result.rowCount !== 1 || !role || ['rolsuper', 'rolcreatedb', 'rolcreaterole', 'rolreplication', 'rolbypassrls', 'has_memberships'].some(flag => role[flag] !== false)) {
    throw new BootstrapError('UNEXPECTED_APPLICATION_ROLE_PRIVILEGES');
  }
  return true;
}

export async function main(env = process.env) {
  const config = readConfiguration(env);
  const [{ SecretsManagerClient, GetSecretValueCommand, PutSecretValueCommand }, pg] = await Promise.all([
    import('@aws-sdk/client-secrets-manager'),
    import('pg'),
  ]);
  const ca = await readFile(config.caPath, 'utf8');
  if (!ca.includes('-----BEGIN CERTIFICATE-----')) throw new BootstrapError('INVALID_CA_BUNDLE');
  const secrets = new SecretsManagerClient({ region: config.region, maxAttempts: 3 });
  async function getSecret(arn, allowMissing = false) {
    try {
      const result = await secrets.send(new GetSecretValueCommand({ SecretId: arn }), { abortSignal: AbortSignal.timeout(45_000) });
      if (typeof result.SecretString !== 'string') throw new BootstrapError('SECRET_MUST_BE_JSON_STRING');
      return result.SecretString;
    } catch (error) {
      if (allowMissing && error?.name === 'ResourceNotFoundException') return null;
      throw error;
    }
  }
  try {
    await bootstrapDatabase(config, {
      readAdminSecret: () => getSecret(config.adminSecretArn),
      readAppSecret: () => getSecret(config.appSecretArn, true),
      writeAppSecret: async SecretString => {
        await secrets.send(new PutSecretValueCommand({ SecretId: config.appSecretArn, SecretString }), { abortSignal: AbortSignal.timeout(45_000) });
      },
      randomPassword: () => randomBytes(48).toString('base64url'),
      connect: async admin => {
        const client = new pg.default.Client({
          host: config.host, port: config.port, database: config.database,
          user: admin.username, password: admin.password,
          ssl: { ca, rejectUnauthorized: true, minVersion: 'TLSv1.2' },
          connectionTimeoutMillis: 10_000, query_timeout: 35_000,
          statement_timeout: 30_000, application_name: 'backstage-db-bootstrap',
        });
        try {
          await client.connect();
          return client;
        } catch (error) {
          await client.end();
          throw error;
        }
      },
      log: message => process.stdout.write(`${message}\n`),
    });
  } finally {
    secrets.destroy();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => {
    // Avoid printing SDK request bodies, PostgreSQL statements, passwords, or
    // raw exception messages. Known error codes identify the failing operation.
    const code = typeof error?.code === 'string' ? error.code : error?.name;
    const safeCode = typeof code === 'string' && /^[A-Za-z0-9_]{1,80}$/.test(code) ? code : 'BOOTSTRAP_FAILED';
    process.stderr.write(`Database bootstrap failed: ${safeCode}. Inspect task configuration, IAM, and database logs.\n`);
    process.exitCode = 1;
  });
}

