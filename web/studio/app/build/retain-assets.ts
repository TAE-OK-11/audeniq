import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// An open Safari tab can still request its previous build's lazy modules after
// deployment. Keep three earlier import graphs, while serving only the new HTML.
const ASSET = /^[\w-]+-[\w-]{8}\.(?:js|css)$/;
const REFERENCES = /\b([\w-]+-[\w-]{8}\.(?:js|css))\b/g;
const MANIFEST = 'compat-entries.json';

export function retainAssets() {
  let directory = '';
  let entries: string[][] = [];
  const saved = new Map<string, Buffer>();
  return {
    name: 'aq-retain-previous-assets',
    apply: 'build' as const,
    configResolved(config: { root: string; build: { outDir: string } }) {
      const out = resolve(config.root, config.build.outDir);
      directory = resolve(out, 'assets');
      if (!existsSync(directory)) return;
      const html = resolve(out, 'index.html');
      const roots = existsSync(html) ? [...readFileSync(html, 'utf8').matchAll(REFERENCES)].map(match => match[1]) : [];
      let previous: unknown = [];
      try { previous = JSON.parse(readFileSync(resolve(directory, MANIFEST), 'utf8')); } catch { /* First build. */ }
      const generations = [roots, ...(Array.isArray(previous) ? previous : [])];
      const seen = new Set<string>();
      for (const raw of generations) {
        const names = (typeof raw === 'string' ? [raw] : Array.isArray(raw) ? raw : [])
          .filter((name): name is string => typeof name === 'string' && ASSET.test(name) && existsSync(resolve(directory, name)));
        const entry = names.find(name => /^index-.*\.js$/.test(name));
        if (!entry || seen.has(entry)) continue;
        seen.add(entry);
        entries.push(names);
        if (entries.length === 4) break;
      }
      const visit = (name: string) => {
        if (saved.has(name) || !ASSET.test(name)) return;
        const path = resolve(directory, name);
        if (!existsSync(path)) return;
        const bytes = readFileSync(path);
        saved.set(name, bytes);
        for (const match of bytes.toString('utf8').matchAll(REFERENCES)) visit(match[1]);
      };
      entries.flat().forEach(visit);
    },
    writeBundle() {
      mkdirSync(directory, { recursive: true });
      const html = readFileSync(resolve(directory, '../index.html'), 'utf8');
      const current = /src="[^" ]*\/assets\/(index-[\w-]{8}\.js)"/.exec(html)?.[1];
      const previous = entries.filter(roots => !roots.includes(current ?? '')).slice(0, 3);
      const keep = new Set<string>();
      const visit = (name: string) => {
        if (keep.has(name) || !saved.has(name)) return;
        keep.add(name);
        for (const match of saved.get(name)!.toString('utf8').matchAll(REFERENCES)) visit(match[1]);
      };
      previous.flat().forEach(visit);
      for (const name of keep) if (!existsSync(resolve(directory, name))) writeFileSync(resolve(directory, name), saved.get(name)!);
      writeFileSync(resolve(directory, MANIFEST), JSON.stringify(previous));
    },
  };
}
