import { describe, expect, it } from "vitest";
import type { BranchProtection } from "@/api/types";
import { mergeRequest, protection } from "@/test/fixtures";
import { approvedMr, compareDiverged, compareReady, compareUpToDate } from "@/test/fixtures.mr";
import { branchRelation, matchingProtections, readiness, requiredApprovals, suggestTitle } from "./model";

const rule = (pattern: string, required_approvals: number): BranchProtection => ({ ...protection, pattern, required_approvals });

describe("branchRelation", () => {
  it("classifies comparisons of target tip -> source tip", () => {
    expect(branchRelation(compareReady)).toBe("fast_forward");
    expect(branchRelation({ ...compareReady, merge_base: undefined })).toBe("fast_forward");
    expect(branchRelation(compareDiverged)).toBe("diverged");
    expect(branchRelation(compareUpToDate)).toBe("up_to_date");
  });
});

describe("protection matching", () => {
  it("applies every matching rule and takes the strictest approval count", () => {
    const rules = [rule("main", 1), rule("release/*", 3), rule("*", 2)];
    expect(matchingProtections(rules, "main").map((r) => r.pattern)).toEqual(["main", "*"]);
    expect(requiredApprovals(matchingProtections(rules, "main"))).toBe(2);
    expect(requiredApprovals(matchingProtections(rules, "release/1.0"))).toBe(3);
    expect(requiredApprovals(matchingProtections(rules, "feature/x"))).toBe(0);
  });
});

describe("suggestTitle", () => {
  it.each([
    ["feature/login-page", "Login page"],
    ["fix_typo", "Fix typo"],
    ["user/bob/docs.readme", "Docs readme"],
    ["", ""],
  ])("%j -> %j", (branch, title) => expect(suggestTitle(branch)).toBe(title));
});

describe("readiness", () => {
  const statuses = (r: ReturnType<typeof readiness>) => r.checks.map((c) => c.status);

  it("is ready when open, approved and fast-forwardable", () => {
    const r = readiness({ mr: approvedMr, required: 2, relation: "fast_forward" });
    expect(statuses(r)).toEqual(["pass", "pass", "pass"]);
    expect(r.blocker).toBeNull();
  });

  it("allows merging a source the target already contains", () => {
    expect(readiness({ mr: approvedMr, required: 0, relation: "up_to_date" }).blocker).toBeNull();
  });

  it("reports the first blocker in precedence order", () => {
    expect(readiness({ mr: mergeRequest, required: 1, relation: "diverged" }).blocker).toBe("1 more approval needed.");
    expect(readiness({ mr: approvedMr, required: 2, relation: "diverged" }).blocker).toMatch(/^Diverged: merge target into source locally/);
    expect(readiness({ mr: approvedMr, required: 2, relation: null }).blocker).toBe("Checking branches…");
    expect(readiness({ mr: approvedMr, required: 2, relation: null, relationProblem: "Source branch gone." }).blocker).toBe(
      "Source branch gone.",
    );
    expect(readiness({ mr: { ...approvedMr, state: "merged" }, required: 0, relation: null }).blocker).toBe(
      "This merge request is merged.",
    );
  });
});
