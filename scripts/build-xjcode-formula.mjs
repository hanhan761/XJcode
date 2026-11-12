// Regenerate the self-contained, offline editor before compiling the launcher.
import fs from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = path.join(root, 'xjcode-native/assets');
const shell = await fs.readFile(path.join(dir, 'formula-shell.html'), 'utf8');
const renderer = await fs.readFile(path.join(root, 'third_party/katex/katex.min.js'), 'utf8');
const editor = await fs.readFile(path.join(dir, 'formula-editor.js'), 'utf8');
const safe = js => js.replace(/<\/script/gi, '<\\/script');
const html = shell.replace('/* XJCODE_KATEX */', () => safe(renderer)).replace('/* XJCODE_EDITOR */', () => safe(editor));
await fs.writeFile(path.join(dir, 'formula.html'), html);
