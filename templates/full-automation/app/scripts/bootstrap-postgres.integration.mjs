import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import test from 'node:test';
import { bootstrapDatabase, parseCredentials, readConfiguration, scramVerifier } from './bootstrap-db.mjs';

// Explicit opt-in against a disposable local PostgreSQL 16 CI service only:
// RUN_POSTGRES_INTEGRATION=1, PGHOST=127.0.0.1, PGPORT=5432,
// PGUSER=postgres, PGPASSWORD=<static CI fixture password>, PGDATABASE=postgres.
// Configure the service with POSTGRES_HOST_AUTH_METHOD=scram-sha-256.
// The database and role names below are fixed test-only names. Existing names
// cause refusal before mutation. No AWS/Secrets Manager calls are made.
// This tests real SQL and password authentication over local transport. It does
// not invoke production main(), exercise production TLS, or emulate RDS itself.
const TEST_DATABASE = 'backstage_bootstrap_test';
const BOOTSTRAP_ROLE = 'backstage_bootstrap_admin_test';
const APP_ROLE = 'backstage_app_test';
const FORBIDDEN_DATABASE = 'backstage_forbidden_database_test';
const FORBIDDEN_ROLE = 'backstage_forbidden_role_test';

test('PostgreSQL 16 restricted bootstrap, SCRAM login, application privileges and rerun', {
  skip: process.env.RUN_POSTGRES_INTEGRATION !== '1',
  timeout: 120_000,
}, async t => {
  assert.equal(process.env.PGHOST, '127.0.0.1', 'integration test requires explicit loopback PGHOST');
  assert.equal(process.env.PGUSER, 'postgres', 'integration test requires the disposable service administrator');
  assert.equal(process.env.PGDATABASE ?? 'postgres', 'postgres', 'administrator connection must use postgres, never an application database');
  const portText = process.env.PGPORT ?? '5432';
  assert.match(portText, /^[0-9]{1,5}$/);
  const port = Number(portText);
  assert.ok(port > 0 && port <= 65535);
  assert.ok(typeof process.env.PGPASSWORD === 'string' && process.env.PGPASSWORD.length > 0, 'set the static CI service password');

  // Import only after opt-in and target checks, so ordinary unit-test runs do
  // not need a database or even an installed pg package to skip this test.
  const { default: pg } = await import('pg');
  const clients = new Set();
  const connection = {
    host: '127.0.0.1', port, ssl: false,
    connectionTimeoutMillis: 10_000, query_timeout: 15_000,
    statement_timeout: 10_000, application_name: 'backstage-bootstrap-integration',
  };
  async function connect(user, password, database) {
    const client = new pg.Client({ ...connection, user, password, database });
    try {
      await client.connect();
      clients.add(client);
      return client;
    } catch (error) {
      await client.end().catch(() => {});
      throw error;
    }
  }
  async function close(client) {
    clients.delete(client);
    await client.end();
  }

  const admin = await connect('postgres', process.env.PGPASSWORD, 'postgres');
  const created = { bootstrap: false, database: false, appMayExist: false, forbiddenDatabase: false, forbiddenRole: false };
  t.after(async () => {
    const failures = [];
    for (const client of clients) {
      if (client !== admin) await close(client).catch(error => failures.push(error));
    }
    const cleanup = [];
    if (created.forbiddenDatabase) cleanup.push(`DROP DATABASE IF EXISTS "${FORBIDDEN_DATABASE}" WITH (FORCE)`);
    if (created.database) cleanup.push(`DROP DATABASE IF EXISTS "${TEST_DATABASE}" WITH (FORCE)`);
    if (created.forbiddenRole) cleanup.push(`DROP ROLE IF EXISTS "${FORBIDDEN_ROLE}"`);
    if (created.appMayExist) cleanup.push(`DROP ROLE IF EXISTS "${APP_ROLE}"`);
    if (created.bootstrap) cleanup.push(`DROP ROLE IF EXISTS "${BOOTSTRAP_ROLE}"`);
    for (const statement of cleanup) await admin.query(statement).catch(error => failures.push(error));
    await close(admin).catch(error => failures.push(error));
    if (failures.length) throw new AggregateError(failures, 'CI PostgreSQL test cleanup failed');
  });

  const version = Number((await admin.query('SHOW server_version_num')).rows[0].server_version_num);
  assert.ok(version >= 160000 && version < 170000, 'this gate specifically requires PostgreSQL 16');
  const administrator = (await admin.query('SELECT rolsuper FROM pg_roles WHERE rolname = current_user')).rows[0];
  assert.equal(administrator.rolsuper, true, 'use the disposable PostgreSQL service, not an RDS or application administrator');
  assert.equal((await admin.query('SELECT 1 FROM pg_database WHERE datname = ANY($1::text[])', [[TEST_DATABASE, FORBIDDEN_DATABASE]])).rowCount, 0, 'refusing to touch an existing test database');
  assert.equal((await admin.query('SELECT 1 FROM pg_roles WHERE rolname = ANY($1::text[])', [[BOOTSTRAP_ROLE, APP_ROLE, FORBIDDEN_ROLE]])).rowCount, 0, 'refusing to touch existing test roles');

  const bootstrapPassword = randomBytes(48).toString('base64url');
  // These privileged attributes are specified by the true CI superuser during
  // CREATE, not by production's restricted bootstrap connection during ALTER.
  await admin.query(`CREATE ROLE "${BOOTSTRAP_ROLE}" WITH LOGIN CREATEDB CREATEROLE NOSUPERUSER NOREPLICATION NOBYPASSRLS PASSWORD '${scramVerifier(bootstrapPassword)}'`);
  created.bootstrap = true;
  await admin.query(`CREATE DATABASE "${TEST_DATABASE}" OWNER "${BOOTSTRAP_ROLE}"`);
  created.database = true;
  const flags = (await admin.query('SELECT rolsuper, rolcreatedb, rolcreaterole, rolreplication, rolbypassrls FROM pg_roles WHERE rolname = $1', [BOOTSTRAP_ROLE])).rows[0];
  assert.deepEqual(flags, { rolsuper: false, rolcreatedb: true, rolcreaterole: true, rolreplication: false, rolbypassrls: false });

  const config = readConfiguration({
    AWS_REGION: 'us-east-1', POSTGRES_HOST: '127.0.0.1', POSTGRES_PORT: portText,
    POSTGRES_DB: TEST_DATABASE, DB_APP_USER: APP_ROLE,
    DB_ADMIN_SECRET_ARN: 'arn:aws:secretsmanager:us-east-1:000000000000:secret:ci-admin',
    DB_APP_SECRET_ARN: 'arn:aws:secretsmanager:us-east-1:000000000000:secret:ci-app',
    PGSSLROOTCERT: '/unused-local-ci-ca.pem',
  });
  let savedSecret = null;
  let secretWrites = 0;
  let passwordGenerations = 0;
  const io = {
    readAdminSecret: async () => JSON.stringify({ username: BOOTSTRAP_ROLE, password: bootstrapPassword }),
    readAppSecret: async () => savedSecret,
    writeAppSecret: async value => { savedSecret = value; secretWrites += 1; },
    randomPassword: () => { passwordGenerations += 1; return randomBytes(48).toString('base64url'); },
    connect: async credentials => {
      const client = await connect(credentials.username, credentials.password, TEST_DATABASE);
      return { query: (...args) => client.query(...args), end: () => close(client) };
    },
    log: () => {},
  };
  created.appMayExist = true;
  await bootstrapDatabase(config, io);
  assert.equal(secretWrites, 1);
  const firstSecret = savedSecret;
  const credentials = parseCredentials(savedSecret, APP_ROLE);
  const stored = (await admin.query("SELECT rolpassword LIKE 'SCRAM-SHA-256$%' AS scram FROM pg_authid WHERE rolname = $1", [APP_ROLE])).rows[0];
  assert.equal(stored.scram, true);
  await assert.rejects(connect(APP_ROLE, randomBytes(48).toString('base64url'), TEST_DATABASE), { code: '28P01' });

  const app = await connect(credentials.username, credentials.password, TEST_DATABASE);
  await app.query('CREATE SCHEMA bootstrap_probe');
  await app.query('CREATE TABLE bootstrap_probe.checks (id integer PRIMARY KEY, value text NOT NULL)');
  await app.query('INSERT INTO bootstrap_probe.checks (id, value) VALUES ($1, $2)', [1, 'schema privileges work']);
  await assert.rejects(async () => {
    await app.query(`CREATE DATABASE "${FORBIDDEN_DATABASE}"`);
    created.forbiddenDatabase = true;
  }, { code: '42501' });
  await assert.rejects(async () => {
    await app.query(`CREATE ROLE "${FORBIDDEN_ROLE}"`);
    created.forbiddenRole = true;
  }, { code: '42501' });
  await close(app);

  await bootstrapDatabase(config, io);
  assert.ok(savedSecret === firstSecret, 'rerun must retain the existing credential, without printing it');
  assert.equal(secretWrites, 1);
  assert.equal(passwordGenerations, 1);
  const afterRerun = await connect(credentials.username, credentials.password, TEST_DATABASE);
  assert.equal((await afterRerun.query('SELECT value FROM bootstrap_probe.checks WHERE id = $1', [1])).rows[0].value, 'schema privileges work');
  await close(afterRerun);
});
