import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import test from 'node:test';
import { bootstrapDatabase, parseCredentials, readConfiguration, scramVerifier } from './bootstrap-db.mjs';

const env = {
  AWS_REGION: 'us-east-1', POSTGRES_HOST: 'database.example.internal', POSTGRES_PORT: '5432',
  POSTGRES_DB: 'backstage', DB_APP_USER: 'backstage_app',
  DB_ADMIN_SECRET_ARN: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:admin',
  DB_APP_SECRET_ARN: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:app',
  PGSSLROOTCERT: '/app/certs/aws-rds-global-bundle.pem',
};
const config = readConfiguration(env);
const password = 'a'.repeat(64);

function fixture({ existing = null, locked = true, failSql = false, roleFlags = {} } = {}) {
  const events = [];
  const statements = [];
  let value = existing;
  let roleExists = existing !== null;
  const io = {
    readAdminSecret: async () => JSON.stringify({ username: 'rds_admin', password }),
    readAppSecret: async () => { events.push('read-app'); return value; },
    writeAppSecret: async next => { events.push('write-app'); value = next; },
    randomPassword: () => password,
    connect: async () => ({
      query: async sql => {
        statements.push(sql);
        events.push(sql.startsWith('ALTER ROLE') ? 'alter-role-redacted' : sql);
        if (sql.startsWith('SELECT pg_try')) return { rows: [{ locked }] };
        if (sql.startsWith('SELECT r.rolsuper')) return {
          rowCount: roleExists ? 1 : 0,
          rows: roleExists ? [{ rolsuper: false, rolcreatedb: false, rolcreaterole: false, rolreplication: false, rolbypassrls: false, has_memberships: false, ...roleFlags }] : [],
        };
        if (sql.startsWith('CREATE ROLE')) roleExists = true;
        if (failSql && sql.startsWith('ALTER ROLE')) throw new Error('simulated database failure');
        return { rowCount: 1 };
      },
      end: async () => { events.push('closed'); },
    }),
    log: message => events.push(message),
  };
  return { io, events, statements, value: () => value };
}

test('first bootstrap stores credentials before creating the login and releases the lock', async () => {
  const f = fixture();
  await bootstrapDatabase(config, f.io);
  assert.deepEqual(JSON.parse(f.value()), { username: 'backstage_app', password });
  assert.ok(f.events.indexOf('write-app') < f.events.indexOf('CREATE ROLE "backstage_app"'));
  assert.equal(f.events.at(-1), 'closed');
  assert.ok(f.events.includes('GRANT CONNECT, CREATE, TEMPORARY ON DATABASE "backstage" TO "backstage_app"'));
  const alter = f.statements.find(sql => sql.startsWith('ALTER ROLE'));
  assert.ok(alter.startsWith("ALTER ROLE \"backstage_app\" WITH LOGIN PASSWORD 'SCRAM-SHA-256$4096:"));
  assert.ok(f.statements.every(sql => !sql.includes(password)), 'SQL must never contain the plaintext application password');
});

test('rerun preserves the existing password without writing another secret version', async () => {
  const prior = JSON.stringify({ username: 'backstage_app', password: 'b'.repeat(64) });
  const f = fixture({ existing: prior });
  await bootstrapDatabase(config, f.io);
  assert.equal(f.value(), prior);
  assert.ok(!f.events.includes('write-app'));
  assert.ok(!f.events.some(event => event.startsWith('CREATE ROLE')));
});

test('concurrent bootstrap fails before any secret mutation', async () => {
  const f = fixture({ locked: false });
  await assert.rejects(bootstrapDatabase(config, f.io), { code: 'BOOTSTRAP_ALREADY_RUNNING' });
  assert.ok(!f.events.includes('read-app'));
  assert.ok(!f.events.includes('write-app'));
  assert.equal(f.events.at(-1), 'closed');
});

test('partial SQL failure leaves saved credentials available for recovery and closes the session', async () => {
  const f = fixture({ failSql: true });
  await assert.rejects(bootstrapDatabase(config, f.io), /simulated database failure/);
  assert.ok(f.value());
  assert.equal(f.events.at(-1), 'closed');
});

test('existing elevated roles or role memberships fail before any role or grant mutation', async () => {
  for (const flag of ['rolsuper', 'rolcreatedb', 'rolcreaterole', 'rolreplication', 'rolbypassrls', 'has_memberships']) {
    const f = fixture({ existing: JSON.stringify({ username: 'backstage_app', password }), roleFlags: { [flag]: true } });
    await assert.rejects(bootstrapDatabase(config, f.io), { code: 'UNEXPECTED_APPLICATION_ROLE_PRIVILEGES' });
    assert.ok(!f.statements.some(sql => /^(CREATE|ALTER|GRANT|REVOKE) /.test(sql)));
    assert.equal(f.events.at(-1), 'closed');
  }
});

test('invalid role identifiers and unexpected app secret identity fail closed', () => {
  assert.throws(() => readConfiguration({ ...env, DB_APP_USER: 'app; DROP ROLE admin' }), { code: 'INVALID_DATABASE_IDENTIFIER' });
  assert.throws(() => parseCredentials(JSON.stringify({ username: 'admin', password }), 'backstage_app'), { code: 'APP_SECRET_USER_OR_PASSWORD_INVALID' });
  assert.throws(() => readConfiguration({ ...env, POSTGRES_PORT: 'NaN' }), { code: 'INVALID_CONNECTION_SETTINGS' });
  for (const invalid of ['a'.repeat(31), 'a'.repeat(129), 'é'.repeat(32), 'a'.repeat(32) + "'", 'a'.repeat(32) + '/']) {
    assert.throws(() => parseCredentials(JSON.stringify({ username: 'backstage_app', password: invalid }), 'backstage_app'), { code: 'APP_SECRET_USER_OR_PASSWORD_INVALID' });
  }
});

test('SCRAM verifier validates both independent RFC 7677 section 3 proofs', () => {
  // Published external vector: https://www.rfc-editor.org/rfc/rfc7677#section-3
  // Neither expected proof is computed by the production verifier function.
  const salt = 'W22ZaJ0SNY7soEsUEjb6gQ==';
  const verifier = scramVerifier('pencil', Buffer.from(salt, 'base64'));
  const [mechanism, parameters, keys] = verifier.split('$');
  assert.equal(mechanism, 'SCRAM-SHA-256');
  assert.equal(parameters, `4096:${salt}`);
  const [storedKey, serverKey] = keys.split(':').map(value => Buffer.from(value, 'base64'));
  assert.equal(storedKey.length, 32);
  assert.equal(serverKey.length, 32);
  const nonce = 'rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0';
  const authMessage = `n=user,r=rOprNGfwEbeRWgbNEkqO,r=${nonce},s=${salt},i=4096,c=biws,r=${nonce}`;
  const signature = createHmac('sha256', serverKey).update(authMessage).digest('base64');
  assert.equal(signature, '6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=');
  const proof = Buffer.from('dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=', 'base64');
  const clientSignature = createHmac('sha256', storedKey).update(authMessage).digest();
  const recoveredClientKey = proof.map((byte, index) => byte ^ clientSignature[index]);
  assert.deepEqual(createHash('sha256').update(recoveredClientKey).digest(), storedKey);
});

test('SCRAM verifier rejects malformed inputs and uses a fresh random salt', () => {
  assert.throws(() => scramVerifier('é', Buffer.alloc(16)), { code: 'INVALID_SCRAM_INPUT' });
  assert.throws(() => scramVerifier(password, Buffer.alloc(8)), { code: 'INVALID_SCRAM_INPUT' });
  assert.notEqual(scramVerifier(password), scramVerifier(password));
});


test('explicit rotation stores a new application password before DB reconciliation', async () => {
  const prior = JSON.stringify({username:'backstage_app',password:'b'.repeat(64)});
  const f=fixture({existing:prior});
  await bootstrapDatabase({...config,rotatePassword:true},f.io);
  assert.equal(JSON.parse(f.value()).password,password);
  assert.notEqual(f.value(),prior);
  assert.ok(f.events.indexOf('write-app')<f.events.indexOf('alter-role-redacted'));
  assert.ok(f.statements.every(sql=>!sql.includes(password)));
});
test('failed rotation retains the selected new credential for a normal recovery rollout', async () => {
  const f=fixture({existing:JSON.stringify({username:'backstage_app',password:'b'.repeat(64)}),failSql:true});
  await assert.rejects(bootstrapDatabase({...config,rotatePassword:true},f.io));
  const saved=f.value();
  const recovered=fixture({existing:saved});
  await bootstrapDatabase(config,recovered.io);
  assert.equal(recovered.value(),saved);
  assert.ok(!recovered.events.includes('write-app'));
});
test('rotation rejects elevated role attributes before changing the application secret', async () => {
  const prior=JSON.stringify({username:'backstage_app',password:'b'.repeat(64)});
  const f=fixture({existing:prior,roleFlags:{rolcreatedb:true}});
  await assert.rejects(bootstrapDatabase({...config,rotatePassword:true},f.io),{code:'UNEXPECTED_APPLICATION_ROLE_PRIVILEGES'});
  assert.equal(f.value(),prior);
  assert.ok(!f.events.includes('write-app'));
});
