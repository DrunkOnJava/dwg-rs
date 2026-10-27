// Drives tools/streaming/wasm_resume.rs under node: a JSON job list in, a JSON
// result list out, a fresh module instance per job.
// Usage: node wasm_resume.mjs <wasm_resume.wasm> <jobs.json>
// A job is {kind, file, digest, chunk, ...}: kind "maps-resume" and
// "section-resume" take a native `checkpoint` path (and the section walk its
// `sidecar`); kinds "maps-run" and "file-run" take `stop` and `checkpoint_out`;
// "file-resume" takes a native `checkpoint` and the cursors `page_crc`,
// `page_len` and `number` the native caller saved; "objects-resume" takes a
// native `checkpoint` and the cursors `digest` and `len`, and "objects-run"
// takes `stop` and `checkpoint_out`; "r2007-run" takes `stop` and
// `checkpoint_out` and "r2007-resume" a native `checkpoint` and the cursors
// `page_crc`, `page_len`, `section` and `id` (a 64-bit page id, as a decimal
// string; "r2007-run" returns it the same way); "r2007-objects-resume" takes a
// native `checkpoint` and the cursors `digest_crc` and `len`, and
// "r2007-objects-run" takes `stop` and `checkpoint_out`.
import { readFileSync, writeFileSync } from 'node:fs';

const [wasmPath, jobsPath] = process.argv.slice(2);
const module = await WebAssembly.compile(readFileSync(wasmPath));
const jobs = JSON.parse(readFileSync(jobsPath, 'utf8'));

function hex(digest) {
  if (!/^[0-9a-f]{64}$/.test(digest)) throw new Error('identity must be 64 hex characters');
  return Uint8Array.from(digest.match(/../g), (b) => parseInt(b, 16));
}

function sidecar(text, wanted) {
  const [head, ...lines] = text.split('\n');
  const [, section, nextRef, pageCrc, pageLen, sectionCrc] = head.split(' ').map(Number);
  const pages = new Map();
  const sections = [];
  for (const line of lines) {
    const f = line.split(' ');
    if (f[0] === 'P') {
      if (!pages.has(Number(f[1]))) pages.set(Number(f[1]), f.slice(1).map(Number));
    } else if (f[0] === 'S') {
      sections.push({ name: f.slice(8).join(' '), refs: [] });
    } else if (f[0] === 'R') {
      sections.at(-1).refs.push(f.slice(1).map(Number));
    }
  }
  return { section: sections[section], nextRef, pageCrc, pageLen, sectionCrc, pages };
}

const results = [];
for (const job of jobs) {
  const { exports: x } = await WebAssembly.instantiate(module, {});
  const base = x.layout(0);
  const at = (which) => base + x.layout(which);
  const file = readFileSync(job.file);
  if (file.length > x.layout(1)) throw new Error('source larger than the kernel scratch');
  const mem = () => new Uint8Array(x.memory.buffer);
  mem().set(file, base);
  mem().set(hex(job.digest), at(4));
  let code;
  if (job.kind === 'maps-resume' || job.kind === 'section-resume') {
    const state = readFileSync(job.checkpoint);
    mem().set(state, at(2));
    if (job.kind === 'maps-resume') {
      code = x.resume_maps(file.length, state.length, job.chunk);
    } else {
      const s = sidecar(readFileSync(job.sidecar, 'utf8'));
      const table = new DataView(x.memory.buffer, at(5), x.layout(6));
      mem().set(new TextEncoder().encode(s.section.name), at(5));
      s.section.refs.forEach((ref, i) => {
        const page = s.pages.get(ref[1]);
        if (!page) throw new Error(`page ${ref[1]} is not in the index`);
        const row = 64 + i * 32;
        table.setInt32(row, page[0], true);
        table.setUint32(row + 4, page[1], true);
        table.setBigUint64(row + 8, BigInt(page[2]), true);
        table.setUint32(row + 16, ref[1], true);
        table.setUint32(row + 20, ref[2], true);
        table.setBigUint64(row + 24, BigInt(ref[3]), true);
      });
      code = x.resume_section(file.length, state.length, job.chunk, s.section.refs.length, s.nextRef,
        s.pageCrc, s.pageLen, s.sectionCrc);
    }
  } else if (job.kind === 'file-resume') {
    const state = readFileSync(job.checkpoint);
    mem().set(state, at(2));
    code = x.resume_file(file.length, state.length, job.chunk, job.page_crc, job.page_len, job.number);
  } else if (job.kind === 'objects-resume') {
    const state = readFileSync(job.checkpoint);
    mem().set(state, at(2));
    code = x.resume_objects(file.length, state.length, job.chunk, job.digest_crc, job.len);
  } else if (job.kind === 'objects-run') {
    code = x.run_objects(file.length, job.stop, job.chunk);
    if (code >= 0) {
      writeFileSync(job.checkpoint_out, mem().slice(at(2), at(2) + x.layout(9)));
    }
  } else if (job.kind === 'r2007-objects-resume') {
    const state = readFileSync(job.checkpoint);
    mem().set(state, at(2));
    code = x.resume_r2007_objects(file.length, state.length, job.chunk, job.digest_crc, job.len);
  } else if (job.kind === 'r2007-objects-run') {
    code = x.run_r2007_objects(file.length, job.stop, job.chunk);
    if (code >= 0) {
      writeFileSync(job.checkpoint_out, mem().slice(at(2), at(2) + x.layout(9)));
    }
  } else if (job.kind === 'r2007-resume') {
    const state = readFileSync(job.checkpoint);
    mem().set(state, at(2));
    code = x.resume_r2007(file.length, state.length, job.chunk, job.page_crc, job.page_len, job.section, BigInt(job.id));
  } else if (job.kind === 'r2007-run') {
    code = x.run_r2007(file.length, job.stop, job.chunk);
    if (code >= 0) {
      writeFileSync(job.checkpoint_out, mem().slice(at(2), at(2) + x.layout(9)));
    }
  } else if (job.kind === 'file-run') {
    code = x.run_file(file.length, job.stop, job.chunk);
    if (code >= 0) {
      writeFileSync(job.checkpoint_out, mem().slice(at(2), at(2) + x.layout(9)));
    }
  } else if (job.kind === 'maps-run') {
    code = x.run_maps(file.length, job.stop, job.chunk);
    if (code >= 0) {
      writeFileSync(job.checkpoint_out, mem().slice(at(2), at(2) + x.layout(9)));
    }
  } else {
    throw new Error(`unknown job kind ${job.kind}`);
  }
  const out = code > 0 ? Buffer.from(mem().slice(at(7), at(7) + code)).toString('utf8') : '';
  const cursors = job.kind === 'file-run' && code >= 0 ? [x.layout(10) >>> 0, x.layout(11) >>> 0, x.layout(12) >>> 0]
    : (job.kind === 'objects-run' || job.kind === 'r2007-objects-run') && code >= 0 ? [x.layout(10) >>> 0, x.layout(11) >>> 0]
    : job.kind === 'r2007-run' && code >= 0 ? [x.layout(10) >>> 0, x.layout(11) >>> 0, x.layout(12) >>> 0, x.r2007_page_id().toString()] : null;
  results.push({ code, out, cursors });
}
process.stdout.write(JSON.stringify(results));
