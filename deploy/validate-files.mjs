#!/usr/bin/env node
// Structural checks for the packaging files (used by deploy/validate.sh).
//
//   node deploy/validate-files.mjs              # YAML/JSON files
//   node deploy/validate-files.mjs --plist F    # XML well-formedness of a plist
//
// YAML parsing uses js-yaml from the npm workspace (node_modules, a
// transitive dependency); without it only JSON files are checked.
import { createRequire } from "node:module";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const errors = [];
const fail = (file, msg) => errors.push(`${file}: ${msg}`);

if (process.argv[2] === "--plist") {
  const file = process.argv[3];
  checkPlist(file, readFileSync(file, "utf8"));
  finish();
}

let yaml = null;
try {
  yaml = createRequire(join(root, "package.json"))("js-yaml");
} catch {
  console.warn("js-yaml not found (run npm ci); YAML files are not parsed");
}

const read = (rel) => readFileSync(join(root, rel), "utf8");
const list = (dir, re) =>
  readdirSync(join(root, dir))
    .filter((f) => re.test(f))
    .map((f) => `${dir}/${f}`);

// ---------------------------------------------------------------- JSON
for (const rel of ["deploy/helm/worktree/values.schema.json"]) {
  try {
    const schema = JSON.parse(read(rel));
    if (schema.type !== "object" || typeof schema.properties !== "object") {
      fail(rel, "top-level schema must be an object schema with properties");
    }
  } catch (e) {
    fail(rel, `invalid JSON: ${e.message}`);
  }
}

if (yaml) {
  const load = (rel) => {
    try {
      return yaml.load(read(rel));
    } catch (e) {
      fail(rel, `invalid YAML: ${e.message}`);
      return undefined;
    }
  };

  // ------------------------------------------------------------ compose
  const composeFile = "deploy/docker-compose.yml";
  const compose = load(composeFile);
  if (compose) {
    const services = compose.services ?? {};
    if (!services.server) fail(composeFile, "missing services.server");
    const profiles = new Set();
    for (const [name, svc] of Object.entries(services)) {
      if (!svc.image && !svc.build) fail(composeFile, `service ${name}: needs image or build`);
      for (const p of svc.profiles ?? []) profiles.add(p);
      for (const dep of Object.keys(svc.depends_on ?? {})) {
        if (!services[dep]) fail(composeFile, `service ${name}: depends_on unknown service ${dep}`);
      }
      for (const v of svc.volumes ?? []) {
        const named = String(v).split(":")[0];
        if (!named.startsWith(".") && !named.startsWith("/") && !(named in (compose.volumes ?? {})) && !named.includes("$")) {
          fail(composeFile, `service ${name}: volume ${named} is not declared`);
        }
      }
    }
    for (const p of ["s3", "smtp"]) {
      if (!profiles.has(p)) fail(composeFile, `missing profile ${p}`);
    }
    if (services.server?.profiles) fail(composeFile, "server must not be behind a profile");
  }

  // ------------------------------------------------------------ workflows
  for (const rel of list(".github/workflows", /\.ya?ml$/)) {
    const wf = load(rel);
    if (!wf) continue;
    if (!wf.on && !wf[true]) fail(rel, "missing on:");
    for (const [id, job] of Object.entries(wf.jobs ?? {})) {
      if (!job["runs-on"] && !job.uses) fail(rel, `job ${id}: missing runs-on`);
      for (const need of [].concat(job.needs ?? [])) {
        if (!wf.jobs[need]) fail(rel, `job ${id}: needs unknown job ${need}`);
      }
      for (const [i, step] of (job.steps ?? []).entries()) {
        if (!step.run && !step.uses) fail(rel, `job ${id} step ${i}: needs run or uses`);
        if (step.uses && !step.uses.startsWith("./") && !/@[\w.-]+$/.test(step.uses)) {
          fail(rel, `job ${id} step ${i}: ${step.uses} must pin a version`);
        }
      }
    }
  }
  load(".github/dependabot.yml");

  // ------------------------------------------------------------ chart
  const chart = load("deploy/helm/worktree/Chart.yaml");
  if (chart) {
    for (const k of ["apiVersion", "name", "version", "appVersion"]) {
      if (!chart[k]) fail("Chart.yaml", `missing ${k}`);
    }
    const cargo = read("Cargo.toml").match(/\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/);
    if (cargo && chart.appVersion !== cargo[1]) {
      fail("deploy/helm/worktree/Chart.yaml", `appVersion ${chart.appVersion} != Cargo.toml version ${cargo[1]}`);
    }
  }
  load("deploy/helm/worktree/values.yaml");
  for (const rel of list("deploy/helm/check/cases", /\.ya?ml$/)) load(rel);
}

finish();

// ---------------------------------------------------------------- plist

function checkPlist(file, text) {
  if (!text.startsWith("<?xml")) fail(file, "must start with an XML declaration");
  // Comments must not contain "--".
  for (const m of text.matchAll(/<!--([\s\S]*?)-->/g)) {
    if (m[1].includes("--")) fail(file, "comment contains --");
  }
  const body = text.replace(/<!--[\s\S]*?-->/g, "").replace(/<\?[\s\S]*?\?>/g, "").replace(/<!DOCTYPE[^>]*>/, "");
  const stack = [];
  for (const m of body.matchAll(/<(\/?)([A-Za-z][\w.-]*)[^>]*?(\/?)>/g)) {
    const [, close, name, self] = m;
    if (self) continue;
    if (close) {
      const top = stack.pop();
      if (top !== name) fail(file, `</${name}> closes <${top ?? "nothing"}>`);
    } else {
      stack.push(name);
    }
  }
  if (stack.length) fail(file, `unclosed <${stack.join("> <")}>`);
  if (!/<key>Label<\/key>\s*<string>[^<]+<\/string>/.test(text)) fail(file, "missing Label");
  if (!/<key>ProgramArguments<\/key>/.test(text)) fail(file, "missing ProgramArguments");
  // <dict> children must alternate key / value.
  if (/<key>[^<]*<\/key>\s*<key>/.test(body)) fail(file, "a <key> is followed by another <key>");
}

function finish() {
  if (errors.length) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("ok");
  process.exit(0);
}
