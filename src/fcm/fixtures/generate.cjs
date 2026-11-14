// Run with Node 24; only the package's restGet is mocked.
const fs = require('node:fs');
const path = require('node:path');
const packageRoot = process.argv[2];
const input = path.resolve(process.argv[3]);
const output = path.resolve(process.argv[4]);
const raw = JSON.parse(fs.readFileSync(input, 'utf8'));
const rest = require(path.join(packageRoot, 'packages/fr-config-common/src/restClient.js'));
rest.restGet = async () => ({ data: { objects: [raw] } });
const { exportManagedObjects } = require(path.join(packageRoot, 'packages/fr-config-pull/src/scripts/managed.js'));
(async () => {
  await exportManagedObjects(output, 'https://tenant.example.com', undefined, false, 'unused');
  // saveJsonToFile queues fs.writeFile; Node drains that work before exiting.
})();
