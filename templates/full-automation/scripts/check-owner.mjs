import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const file = fileURLToPath(new URL('../app/catalog/org.yaml', import.meta.url));
const text = (await readFile(file, 'utf8')).replace(/^\s*#.*$/gm, '');
// Actual parsing and entity validation occurs in Backstage. This build guard
// prevents accidentally shipping the documented unusable seed identity.
if (/REPLACE|CHANGEME|YOUR_GITHUB|github\.com\/user-id:\s*['"]?0(?:['"]?\s|$)/i.test(text)) {
  process.stderr.write('Edit app/catalog/org.yaml with your GitHub numeric user ID and operator membership before building.\n');
  process.exitCode = 1;
} else if (!/github\.com\/user-id:\s*(["'])[1-9][0-9]*\1\s*(?:#.*)?$/m.test(text)) {
  process.stderr.write('Set github.com/user-id to your numeric GitHub ID as a QUOTED YAML string in app/catalog/org.yaml.\n');
  process.exitCode = 1;
}

