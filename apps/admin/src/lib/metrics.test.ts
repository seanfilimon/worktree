import { describe, expect, it } from "vitest";
import { metricsText } from "@/test/fixtures.dashboard";
import { histogramQuantile, metricValue, parsePrometheus, requestRate, requestTotals, samplesOf } from "./metrics";

describe("parsePrometheus", () => {
  it("parses families, types, help and labelled samples", () => {
    const families = parsePrometheus(metricsText());
    const requests = families.get("worktree_http_requests_total");
    expect(requests?.type).toBe("counter");
    expect(requests?.help).toBe("Total HTTP requests by route and status code.");
    expect(requests?.samples).toEqual([
      { name: "worktree_http_requests_total", labels: { route: "GET /api/v1/tenants", code: "200" }, value: 98 },
      { name: "worktree_http_requests_total", labels: { route: "GET /api/v1/tenants", code: "500" }, value: 2 },
    ]);
    expect(metricValue(families, "go_goroutines")).toBe(42);
    expect(samplesOf(families, "worktree_build_info")[0]?.labels).toEqual({ version: "2.0.0-test", goversion: "go1.25.0" });
  });

  it("groups histogram sub-samples under their family and reads +Inf", () => {
    const families = parsePrometheus(metricsText());
    const hist = families.get("worktree_http_request_duration_seconds");
    expect(hist?.type).toBe("histogram");
    expect(hist?.samples.map((s) => s.name)).toEqual([
      ...Array<string>(4).fill("worktree_http_request_duration_seconds_bucket"),
      "worktree_http_request_duration_seconds_sum",
      "worktree_http_request_duration_seconds_count",
    ]);
    expect(families.has("worktree_http_request_duration_seconds_bucket")).toBe(false);
    expect(hist?.samples[3]?.labels.le).toBe("+Inf");
  });

  it("handles escapes, timestamps, special values, CRLF and junk lines", () => {
    const families = parsePrometheus(
      [
        "# HELP odd A help with a \\\\ backslash\\nand newline",
        "# TYPE odd gauge",
        'odd{path="C:\\\\data",msg="say \\"hi\\"",nl="a\\nb"} -Inf 1700000000000',
        "untyped_one NaN",
        "not a sample line {",
        "bad_value abc",
        'broken{label="unterminated} 1',
        "",
      ].join("\r\n"),
    );
    const odd = families.get("odd");
    expect(odd?.help).toBe("A help with a \\ backslash\nand newline");
    expect(odd?.samples[0]).toEqual({
      name: "odd",
      labels: { path: "C:\\data", msg: 'say "hi"', nl: "a\nb" },
      value: -Infinity,
      timestamp: 1700000000000,
    });
    expect(families.get("untyped_one")?.type).toBe("untyped");
    expect(metricValue(families, "untyped_one")).toBeNaN();
    expect(families.has("bad_value")).toBe(false);
    expect(families.has("broken")).toBe(false);
    expect(families.has("not")).toBe(false);
  });

  it("returns an empty map for empty input", () => {
    expect(parsePrometheus("").size).toBe(0);
  });
});

describe("request metrics", () => {
  it("sums request totals across routes and codes", () => {
    const totals = requestTotals(parsePrometheus(metricsText()));
    expect(totals).toEqual({
      requests: 100,
      errors: 2,
      durationSum: 1.5,
      durationCount: 100,
      buckets: [
        [0.005, 60],
        [0.01, 90],
        [0.1, 98],
        [Infinity, 100],
      ],
    });
  });

  it("interpolates histogram quantiles", () => {
    const buckets: [number, number][] = [
      [0.1, 50],
      [0.2, 100],
      [Infinity, 100],
    ];
    expect(histogramQuantile(0.5, buckets)).toBeCloseTo(0.1);
    expect(histogramQuantile(0.75, buckets)).toBeCloseTo(0.15);
    expect(histogramQuantile(0.25, buckets)).toBeCloseTo(0.05);
    expect(histogramQuantile(0.99, [[0.1, 1], [Infinity, 100]])).toBe(0.1);
    expect(histogramQuantile(0.5, [])).toBeUndefined();
    expect(histogramQuantile(0.5, [[Infinity, 0]])).toBeUndefined();
  });

  it("derives rates and latency between two scrapes", () => {
    const prev = requestTotals(parsePrometheus(metricsText()));
    const next = requestTotals(
      parsePrometheus(metricsText({ requests: 130, errors: 5, sum: 1.8, buckets: [80, 115, 128, 130] })),
    );
    const point = requestRate(prev, 0, next, 15_000);
    expect(point.t).toBe(15_000);
    expect(point.rps).toBeCloseTo(2);
    expect(point.errorsPerSec).toBeCloseTo(0.2);
    expect(point.avgMs).toBeCloseTo(10);
    // 28.5th of 30 interval requests falls in the (0.01, 0.1] bucket.
    expect(point.p95Ms).toBeGreaterThan(10);
    expect(point.p95Ms).toBeLessThanOrEqual(100);
  });

  it("treats a counter reset as a fresh start and reports no latency without traffic", () => {
    const prev = requestTotals(parsePrometheus(metricsText()));
    const restarted = requestTotals(parsePrometheus(metricsText({ requests: 10, errors: 0, sum: 0.1, buckets: [10, 10, 10, 10] })));
    const reset = requestRate(prev, 0, restarted, 10_000);
    expect(reset.rps).toBeCloseTo(1);
    expect(reset.avgMs).toBeCloseTo(10);

    const idle = requestRate(prev, 0, prev, 10_000);
    expect(idle.rps).toBe(0);
    expect(idle.avgMs).toBeUndefined();
    expect(idle.p95Ms).toBeUndefined();
  });
});
