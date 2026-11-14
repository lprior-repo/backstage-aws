const { randomBytes } = require('node:crypto');

async function bootstrapDatabase(database, saveSecret) {
  await database.query('BEGIN');
  try {
    const existing = await database.query(
      'SELECT 1 FROM pg_roles WHERE rolname = $1',
      ['backstage_app'],
    );
    if (existing.rowCount !== 0) {
      throw new Error(
        'Application role already exists; bootstrap cannot rotate it',
      );
    }
    // The generated alphabet cannot contain an SQL quote or escape character.
    const password = randomBytes(48).toString('base64url');
    await database.query(
      `CREATE ROLE backstage_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT PASSWORD '${password}'`,
    );
    await database.query(
      'GRANT CONNECT, CREATE, TEMPORARY ON DATABASE backstage TO backstage_app',
    );
    await saveSecret({ username: 'backstage_app', password });
    await database.query('COMMIT');
  } catch (error) {
    await database.query('ROLLBACK');
    throw error;
  }
}

async function main() {
  const { Client } = require('pg');
  const { readFileSync } = require('node:fs');
  const {
    SecretsManagerClient,
    PutSecretValueCommand,
  } = require('@aws-sdk/client-secrets-manager');
  for (const name of [
    'POSTGRES_HOST',
    'POSTGRES_USER',
    'POSTGRES_PASSWORD',
    'APP_DATABASE_SECRET_ARN',
    'RDS_CA_BUNDLE',
    'AWS_REGION',
  ]) {
    if (!process.env[name]) throw new Error(`Missing ${name}`);
  }
  const database = new Client({
    host: process.env.POSTGRES_HOST,
    port: 5432,
    database: 'backstage',
    user: process.env.POSTGRES_USER,
    password: process.env.POSTGRES_PASSWORD,
    ssl: {
      rejectUnauthorized: true,
      ca: readFileSync(process.env.RDS_CA_BUNDLE, 'utf8'),
    },
  });
  const secrets = new SecretsManagerClient({ region: process.env.AWS_REGION });
  try {
    await database.connect();
    await bootstrapDatabase(database, credentials =>
      secrets.send(
        new PutSecretValueCommand({
          SecretId: process.env.APP_DATABASE_SECRET_ARN,
          SecretString: JSON.stringify(credentials),
        }),
      ),
    );
    console.log('Database application role and secret created');
  } finally {
    await database.end();
    secrets.destroy();
  }
}

module.exports = { bootstrapDatabase };
if (require.main === module) {
  main().catch(() => {
    console.error(
      'Database bootstrap failed; application service must remain stopped',
    );
    process.exitCode = 1;
  });
}
