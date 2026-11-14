const { test } = require('node:test');
const assert = require('node:assert/strict');
const { bootstrapDatabase } = require('./bootstrap-database.cjs');

function database({ existing = false, failGrant = false } = {}) {
  const queries = [];
  return {
    queries,
    async query(sql, args) {
      queries.push({ sql, args });
      if (sql.startsWith('SELECT')) return { rowCount: existing ? 1 : 0 };
      if (failGrant && sql.startsWith('GRANT'))
        throw new Error('permission denied');
      return {};
    },
  };
}

test('creates a restricted login and publishes its matching password before commit', async () => {
  const db = database();
  let credentials;
  await bootstrapDatabase(db, async value => {
    assert.equal(db.queries.at(-1).sql.startsWith('GRANT'), true);
    credentials = value;
  });
  assert.equal(credentials.username, 'backstage_app');
  assert.match(credentials.password, /^[A-Za-z0-9_-]{64}$/);
  const create = db.queries.find(q => q.sql.startsWith('CREATE ROLE')).sql;
  assert.match(create, /NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT/);
  assert.ok(create.endsWith(`PASSWORD '${credentials.password}'`));
  assert.equal(db.queries.at(-1).sql, 'COMMIT');
});

test('refuses to change an existing application login', async () => {
  const db = database({ existing: true });
  let writes = 0;
  await assert.rejects(
    bootstrapDatabase(db, async () => writes++),
    /already exists/,
  );
  assert.equal(writes, 0);
  assert.equal(
    db.queries.some(q => q.sql.startsWith('CREATE')),
    false,
  );
  assert.equal(db.queries.at(-1).sql, 'ROLLBACK');
});

test('rolls back database creation if publishing the password fails', async () => {
  const db = database();
  await assert.rejects(
    bootstrapDatabase(db, async () => {
      throw new Error('AWS unavailable');
    }),
    /AWS unavailable/,
  );
  assert.equal(db.queries.at(-1).sql, 'ROLLBACK');
  assert.equal(
    db.queries.some(q => q.sql === 'COMMIT'),
    false,
  );
});

test('does not publish credentials if database grants fail', async () => {
  const db = database({ failGrant: true });
  let writes = 0;
  await assert.rejects(
    bootstrapDatabase(db, async () => writes++),
    /permission denied/,
  );
  assert.equal(writes, 0);
  assert.equal(db.queries.at(-1).sql, 'ROLLBACK');
});
