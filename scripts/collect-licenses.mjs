// Copy license texts for the active Linux dependency graph, including fonts.
import { mkdirSync, readdirSync, copyFileSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { resolve, join, basename } from 'node:path';

const output = resolve(process.argv[2]);
mkdirSync(output, { recursive: true });
const metadata = JSON.parse(readFileSync(process.argv[3], 'utf8'));
const active = new Set(readFileSync(process.argv[4], 'utf8').split('\n').map(s => s.replace(/ \(\*\)$/, '')));
const packages = metadata.packages.filter(p => active.has(`${p.name} v${p.version}`) && p.source);
const lines = ['Linux Rust build dependencies', ''];
for (const pkg of packages) {
  lines.push(`${pkg.name} ${pkg.version} | ${pkg.license ?? 'See license file'} | ${pkg.repository ?? ''} | ${(pkg.authors ?? []).join(', ')}`);
  const directory = resolve(pkg.manifest_path, '..');
  const dest = join(output, `${pkg.name}-${pkg.version}`);
  const copy = (base, depth = 0) => {
    for (const entry of readdirSync(base, { withFileTypes: true })) {
      const full = join(base, entry.name);
      if (entry.isDirectory() && depth < 2 && /licen[cs]e|font/i.test(entry.name)) copy(full, depth + 1);
      if (entry.isFile() && (/licen[cs]e|^copying|^copyright|^notice|^ofl|^ufl/i.test(entry.name) || (depth > 0 && /\.txt$/i.test(entry.name)))) {
        mkdirSync(dest, { recursive: true });
        copyFileSync(full, join(dest, `${depth}-${basename(full)}`));
      }
    }
  };
  copy(directory);
  if (pkg.license_file) {
    mkdirSync(dest, { recursive: true });
    copyFileSync(join(directory, pkg.license_file), join(dest, basename(pkg.license_file)));
  }
}
writeFileSync(join(output, 'PACKAGES.txt'), lines.join('\n') + '\n');
const sysroot = process.argv[5];
const rustLicenses = join(sysroot, 'share', 'doc', 'rust', 'licenses');
if (existsSync(rustLicenses)) {
  mkdirSync(join(output, 'rust-standard-library'), { recursive: true });
  for (const name of readdirSync(rustLicenses)) {
    copyFileSync(join(rustLicenses, name), join(output, 'rust-standard-library', name));
  }
}
console.log(`License notices collected for ${packages.length} packages.`);
