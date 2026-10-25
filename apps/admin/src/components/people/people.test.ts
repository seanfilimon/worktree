import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as px from "@/test/fixtures.people";
import { initials } from "./initials";
import { generatePassword, passwordStrength } from "./password";
import { roleAllows } from "./roles";
import { sortTreeRows, type TreeRow } from "./treeSort";
import { SLUG_RE, slugify } from "./validation";

describe("slugify", () => {
  it.each([
    ["Acme Corp", "acme-corp"],
    ["  Ünïcödé  Ltd. ", "unicode-ltd"],
    ["--x--", "x"],
    ["!!!", ""],
  ])("%j → %j", (name, slug) => {
    expect(slugify(name)).toBe(slug);
  });

  it("produces valid slugs of at most 63 characters", () => {
    const slug = slugify(`${"a".repeat(62)} b`);
    expect(slug.length).toBeLessThanOrEqual(63);
    expect(SLUG_RE.test(slug)).toBe(true);
  });
});

describe("passwordStrength", () => {
  it.each([
    ["short", 0, "Too short"],
    ["aaaaaaaaaaaaaaaa", 1, "Weak"],
    ["password", 1, "Weak"],
    ["longerpassword", 2, "Fair"],
    ["Passw0rd-ok", 2, "Fair"],
    ["Passw0rd-okay", 3, "Good"],
    ["Correct-Horse-9-Battery", 4, "Strong"],
  ])("%j scores %i", (pw, score, label) => {
    expect(passwordStrength(pw)).toEqual({ score, label });
  });
});

describe("generatePassword", () => {
  it("has the requested length, every class and no look-alikes", () => {
    for (let i = 0; i < 50; i++) {
      const pw = generatePassword(16);
      expect(pw).toHaveLength(16);
      expect(pw).toMatch(/[a-z]/);
      expect(pw).toMatch(/[A-Z]/);
      expect(pw).toMatch(/[0-9]/);
      expect(pw).toMatch(/[^A-Za-z0-9]/);
      expect(pw).not.toMatch(/[0O1lI]/);
    }
    expect(generatePassword()).not.toBe(generatePassword());
  });
});

describe("initials", () => {
  it.each([
    ["Alice Admin", "AA"],
    ["bob", "BO"],
    ["acme-corp", "AC"],
    ["", "?"],
  ])("%j → %j", (name, out) => {
    expect(initials(name)).toBe(out);
  });
});

describe("roleAllows", () => {
  it("is cumulative", () => {
    expect(roleAllows("owner", "viewer")).toBe(true);
    expect(roleAllows("maintainer", "maintainer")).toBe(true);
    expect(roleAllows("developer", "maintainer")).toBe(false);
  });
});

describe("sortTreeRows", () => {
  const rows: TreeRow[] = [
    ...px.acmeTrees.map((summary) => ({ tenant: fx.tenant.slug, summary })),
    ...px.globexTrees.map((summary) => ({ tenant: px.globex.slug, summary })),
  ];
  const names = (r: TreeRow[]) => r.map((x) => `${x.tenant}/${x.summary.tree.name}`);

  it("sorts by name, snapshots and updated in both directions", () => {
    expect(names(sortTreeRows(rows, { key: "name", dir: "asc" }))).toEqual(["acme/api", "acme/web", "globex/docs"]);
    expect(names(sortTreeRows(rows, { key: "name", dir: "desc" }))).toEqual(["globex/docs", "acme/web", "acme/api"]);
    expect(names(sortTreeRows(rows, { key: "snapshots", dir: "desc" }))).toEqual(["acme/api", "globex/docs", "acme/web"]);
    expect(names(sortTreeRows(rows, { key: "updated", dir: "asc" }))).toEqual(["acme/web", "globex/docs", "acme/api"]);
  });

  it("does not mutate its input", () => {
    const before = names(rows);
    sortTreeRows(rows, { key: "snapshots", dir: "asc" });
    expect(names(rows)).toEqual(before);
  });
});
