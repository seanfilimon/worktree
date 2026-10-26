import { describe, expect, it } from "vitest";
import { cleanStatement, parseStatementsJson } from "@/pages/iam/util";
import {
  cleanAttributeConditions,
  conditionText,
  normalizeCondition,
  overridesToContext,
  parseAttributeConditions,
  rowsFromAttributes,
  rowsToAttributes,
  sameAttributes,
  splitConditionKey,
  validateAttributeKey,
  validateAttributeValue,
  validateCondition,
  validateOverrides,
  validateRows,
  validateScopedKey,
  valuesForOp,
} from "./abac";

describe("attribute keys and values", () => {
  it("accepts keys matching ^[a-z][a-z0-9_.-]{0,63}$", () => {
    for (const k of ["a", "department", "cost_center", "geo.region", "x-1", `a${"b".repeat(63)}`]) {
      expect(validateAttributeKey(k)).toBeUndefined();
    }
    for (const k of ["", "1abc", "Dept", "_x", "a b", `a${"b".repeat(64)}`, "dé"]) {
      expect(validateAttributeKey(k)).toBeDefined();
    }
  });

  it("limits values to 1..256 bytes without control characters", () => {
    expect(validateAttributeValue("eng")).toBeUndefined();
    expect(validateAttributeValue("x".repeat(256))).toBeUndefined();
    expect(validateAttributeValue("")).toBe("Enter a value.");
    expect(validateAttributeValue("x".repeat(257))).toBe("At most 256 bytes.");
    // 129 two-byte characters = 258 bytes.
    expect(validateAttributeValue("é".repeat(129))).toBe("At most 256 bytes.");
    expect(validateAttributeValue("a\tb")).toBe("No control characters.");
    expect(validateAttributeValue("a\u0085b")).toBe("No control characters.");
  });
});

describe("attribute rows", () => {
  it("converts maps to sorted rows and back", () => {
    const rows = rowsFromAttributes({ team: "web", department: "eng" });
    expect(rows).toEqual([
      { key: "department", value: "eng" },
      { key: "team", value: "web" },
    ]);
    expect(rowsToAttributes([...rows, { key: " region ", value: "eu" }])).toEqual({ department: "eng", team: "web", region: "eu" });
  });

  it("reports per-row, duplicate and count errors", () => {
    const v = validateRows([
      { key: "dept", value: "eng" },
      { key: "Bad", value: "" },
      { key: "dept", value: "ops" },
    ]);
    expect(v.valid).toBe(false);
    expect(v.rows[0]).toEqual({});
    expect(v.rows[1]).toEqual({ key: expect.stringContaining("Invalid key") as string, value: "Enter a value." });
    expect(v.rows[2]).toEqual({ key: "Duplicate key." });

    const many = Array.from({ length: 65 }, (_, i) => ({ key: `k${i}`, value: "v" }));
    expect(validateRows(many).general).toEqual(["At most 64 attributes."]);
    expect(validateRows(many.slice(0, 64)).valid).toBe(true);
  });

  it("compares maps regardless of key order", () => {
    expect(sameAttributes({ a: "1", b: "2" }, { b: "2", a: "1" })).toBe(true);
    expect(sameAttributes({ a: "1" }, { a: "2" })).toBe(false);
    expect(sameAttributes({ a: "1" }, { a: "1", b: "2" })).toBe(false);
  });
});

describe("attribute conditions", () => {
  it("splits and validates scoped keys", () => {
    expect(splitConditionKey("principal.department")).toEqual({ scope: "principal", name: "department" });
    expect(splitConditionKey("tree.geo.region")).toEqual({ scope: "tree", name: "geo.region" });
    expect(splitConditionKey("account.x")).toEqual({ scope: "", name: "account.x" });
    expect(validateScopedKey("tenant.region")).toBeUndefined();
    expect(validateScopedKey("account.x")).toMatch(/principal\., tree\. or tenant\./);
    expect(validateScopedKey("principal.")).toBe("Enter a key.");
    expect(validateScopedKey("tree.Bad")).toMatch(/Invalid key/);
  });

  it("checks value counts per operator", () => {
    expect(validateCondition({ key: "principal.dept", op: "equals", values: ["eng"] })).toEqual({});
    expect(validateCondition({ key: "principal.dept", op: "equals", values: ["a", "b"] }).values).toBe("Enter exactly one value.");
    expect(validateCondition({ key: "principal.dept", op: "not_equals", values: [] }).values).toBe("Enter exactly one value.");
    expect(validateCondition({ key: "principal.dept", op: "in", values: [] }).values).toBe("Add at least one value.");
    expect(validateCondition({ key: "principal.dept", op: "in", values: Array.from({ length: 65 }, (_, i) => `v${i}`) }).values).toBe(
      "At most 64 values.",
    );
    expect(validateCondition({ key: "tree.class", op: "exists" })).toEqual({});
    expect(validateCondition({ key: "tree.class", op: "not_exists", values: ["x"] }).values).toBe("This operator takes no values.");
    expect(validateCondition({ key: "tree.class", op: "matches", values: ["x"] }).op).toBe("Choose an operator.");
  });

  it("normalises values per operator", () => {
    expect(normalizeCondition({ key: "tree.c", op: "exists", values: ["stale"] })).toEqual({ key: "tree.c", op: "exists" });
    expect(normalizeCondition({ key: "tree.c", op: "equals", values: ["a", "b"] })).toEqual({ key: "tree.c", op: "equals", values: ["a"] });
    expect(normalizeCondition({ key: "tree.c", op: "in" })).toEqual({ key: "tree.c", op: "in", values: [] });
    expect(valuesForOp("not_exists", ["a"])).toBeUndefined();
    expect(valuesForOp("starts_with", ["a", "b"])).toEqual(["a", "b"]);
    expect(valuesForOp("not_equals", ["a", "b"])).toEqual(["a"]);
    expect(cleanAttributeConditions([{ key: "tenant.r", op: "not_exists", values: [] }])).toEqual([{ key: "tenant.r", op: "not_exists" }]);
  });

  it("parses JSON input leniently", () => {
    expect(parseAttributeConditions([{ key: "principal.d", op: "in", values: ["a", 1, "b"] }, "junk", { op: "exists" }])).toEqual([
      { key: "principal.d", op: "in", values: ["a", "b"] },
      { key: "", op: "" },
      { key: "", op: "exists" },
    ]);
  });

  it("renders conditions as text", () => {
    expect(conditionText({ key: "principal.department", op: "in", values: ["eng", "ops"] })).toBe("principal.department in [eng, ops]");
    expect(conditionText({ key: "tree.classification", op: "equals", values: ["secret"] })).toBe("tree.classification equals secret");
    expect(conditionText({ key: "tenant.region", op: "exists", values: null })).toBe("tenant.region exists");
  });
});

describe("policy statements with attribute conditions", () => {
  const base = { effect: "allow" as const, actions: ["tree:read"], resources: ["*"], principals: ["*"] };

  it("serialises statements without attribute conditions exactly as before", () => {
    expect(cleanStatement({ ...base, conditions: {} })).toEqual({ ...base, conditions: {} });
    expect(Object.keys(cleanStatement({ ...base, conditions: { attributes: [] } }).conditions)).toEqual([]);
    const parsed = parseStatementsJson(JSON.stringify([{ ...base, conditions: { attributes: [] } }]));
    expect(parsed.ok && parsed.statements[0]?.conditions).toEqual({});
  });

  it("round-trips conditions.attributes through the JSON editor", () => {
    const statements = [
      {
        ...base,
        conditions: {
          require_mfa: true,
          attributes: [
            { key: "principal.department", op: "in", values: ["eng", "ops"] },
            { key: "tree.classification", op: "not_exists" },
            { key: "tenant.region", op: "equals", values: ["eu"] },
          ],
        },
      },
    ];
    const parsed = parseStatementsJson(JSON.stringify(statements));
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.statements.map(cleanStatement)).toEqual(statements);
  });
});

describe("simulator overrides", () => {
  it("validates scoped keys and values unless unset", () => {
    expect(
      validateOverrides([
        { key: "principal.department", value: "eng", unset: false },
        { key: "tree.classification", value: "", unset: true },
        { key: "department", value: "x", unset: false },
        { key: "principal.department", value: "", unset: false },
      ]),
    ).toEqual([
      {},
      {},
      { key: expect.stringMatching(/principal\., tree\. or tenant\./) as string },
      { key: "Duplicate key.", value: "Enter a value." },
    ]);
  });

  it("builds context.attributes with null for unset", () => {
    expect(overridesToContext([])).toBeUndefined();
    expect(
      overridesToContext([
        { key: " principal.department ", value: "eng", unset: false },
        { key: "tree.classification", value: "ignored", unset: true },
      ]),
    ).toEqual({ "principal.department": "eng", "tree.classification": null });
  });
});
