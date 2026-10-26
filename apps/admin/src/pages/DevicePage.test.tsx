import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { formatUserCodeInput, normalizeUserCode, type DeviceRequest } from "@/api/device";
import { apiError, json } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const CODE = "BCDF-GHJK";
const BASE = `/api/v1/auth/device/${CODE}`;

function request(overrides: Partial<DeviceRequest> = {}): DeviceRequest {
  return {
    user_code: CODE,
    client_id: "wt-cli",
    client_name: "wt CLI on laptop",
    scopes: ["tree:read", "mr:*"],
    status: "pending",
    remote_ip: "198.51.100.20",
    user_agent: "worktree-sdk/0.8.0",
    created_at: new Date(Date.now() - 60_000).toISOString(),
    expires_at: new Date(Date.now() + 540_000).toISOString(),
    mfa_required: false,
    ...overrides,
  };
}

describe("device sign-in page", () => {
  it("prefills the code from verification_uri_complete, shows the request and approves it", async () => {
    const { user, fetchMock } = renderApp({
      route: "/device?code=bcdfghjk",
      api: {
        [`GET ${BASE}`]: request(),
        [`POST ${BASE}/approve`]: request({ status: "approved" }),
      },
    });
    const input = await screen.findByLabelText("Device code");
    expect(input).toHaveValue(CODE);
    expect(fetchMock.find(`GET ${BASE}`)).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Continue" }));

    expect(await screen.findByText("Approve this device?")).toBeInTheDocument();
    expect(screen.getByText("wt CLI on laptop")).toBeInTheDocument();
    expect(within(screen.getByLabelText("Device request")).getByText(CODE)).toBeInTheDocument();
    const scopes = screen.getByRole("list", { name: "Requested scopes" });
    expect(within(scopes).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["tree:read", "mr:*"]);
    expect(screen.getByText(/198\.51\.100\.20/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Two-factor code")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByText("Device approved")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${BASE}/approve`)[0]?.body).toEqual({});
    expect(fetchMock.find(`POST ${BASE}/deny`)).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Enter another code" }));
    expect(await screen.findByLabelText("Device code")).toHaveValue("");
  });

  it("asks for a second factor when the session has none", async () => {
    const { user, fetchMock } = renderApp({
      route: "/device",
      api: {
        [`GET ${BASE}`]: request({ mfa_required: true, scopes: null }),
        [`POST ${BASE}/approve`]: (req) =>
          (req.body as { code?: string }).code === "123456"
            ? json(request({ status: "approved" }))
            : apiError(403, "forbidden", "invalid authentication code"),
      },
    });
    await user.type(await screen.findByLabelText("Device code"), "bcdf ghjk");
    expect(screen.getByLabelText("Device code")).toHaveValue(CODE);
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Full access to your account")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(screen.getByText(/Enter the 6-digit code/)).toBeInTheDocument();
    expect(fetchMock.find(`POST ${BASE}/approve`)).toHaveLength(0);

    await user.type(screen.getByLabelText("Two-factor code"), "000000");
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByText("invalid authentication code")).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Two-factor code"));
    await user.type(screen.getByLabelText("Two-factor code"), "123 456");
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByText("Device approved")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${BASE}/approve`).at(-1)?.body).toEqual({ code: "123456" });
  });

  it("denies a request", async () => {
    const { user, fetchMock } = renderApp({
      route: `/device?code=${CODE}`,
      api: {
        [`GET ${BASE}`]: request(),
        [`POST ${BASE}/deny`]: request({ status: "denied" }),
      },
    });
    await user.click(await screen.findByRole("button", { name: "Continue" }));
    await user.click(await screen.findByRole("button", { name: "Deny" }));
    expect(await screen.findByText("Sign-in denied")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${BASE}/deny`)).toHaveLength(1);
    expect(fetchMock.find(`POST ${BASE}/approve`)).toHaveLength(0);
  });

  it("validates the code and explains unknown or decided codes", async () => {
    const { user, fetchMock } = renderApp({
      route: "/device",
      api: {
        [`GET ${BASE}`]: apiError(404, "not_found", "unknown or expired device code"),
        "GET /api/v1/auth/device/BBBB-CCCC": request({ user_code: "BBBB-CCCC", status: "approved" }),
      },
    });
    const input = await screen.findByLabelText("Device code");
    await user.type(input, "ABCD");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText(/Enter the 8-letter code/)).toBeInTheDocument();
    expect(fetchMock.calls.some((c) => c.url.pathname.startsWith("/api/v1/auth/device/"))).toBe(false);

    await user.clear(input);
    await user.type(input, CODE);
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Code not found")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Enter another code" }));
    await user.type(await screen.findByLabelText("Device code"), "BBBBCCCC");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Device approved")).toBeInTheDocument();
  });
});

describe("user codes", () => {
  it("normalizes and formats codes like the server", () => {
    expect(normalizeUserCode("bcdf-ghjk")).toBe(CODE);
    expect(normalizeUserCode(" BCDF GHJK ")).toBe(CODE);
    expect(normalizeUserCode("BCDF-GHJA")).toBeNull(); // vowel
    expect(normalizeUserCode("BCDF-GHJ")).toBeNull();
    expect(formatUserCodeInput("bcdfg")).toBe("BCDF-G");
    expect(formatUserCodeInput("bc-df 12gh jkl")).toBe("BCDF-GHJK");
  });
});
