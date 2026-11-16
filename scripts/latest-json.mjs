import { readFileSync } from 'node:fs';

const [ver, notesFile, sigFile, url] = process.argv.slice(2);
const die = (m) => { console.error(`latest-json: ${m}`); process.exit(1); };
if (!/^\d+\.\d+\.\d+$/.test(ver ?? '')) die('bad version');
const signature = readFileSync(sigFile, 'utf8').trim();
if (!signature) die('empty signature');
if (!/^https:\/\/api\.github\.com\/repos\/Sashanator\/conclave\/releases\/assets\/\d+$/.test(url ?? '')) die('url is not a Conclave release asset API url');
const notes = readFileSync(notesFile, 'utf8').trim();
console.log(JSON.stringify({ version: ver, notes, pub_date: new Date().toISOString(), platforms: { 'windows-x86_64': { signature, url } } }, null, 2));
