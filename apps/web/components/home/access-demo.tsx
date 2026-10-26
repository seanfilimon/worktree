"use client";

import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { RiCheckLine, RiCloseLine, RiShieldCheckLine } from "react-icons/ri";
import { cx } from "@/utils/cx";
import { DiagramFrame } from "./frame";
import { ease } from "./motion";

/**
 * Built-in roles against common actions, with two branch protections to switch on. The minimum role per action
 * is the server's built-in matrix (services/server-go internal/authz/actions.go, actionMinRole); protections are
 * checked after roles and refuse even owners unless they push with --bypass-protection (no_force_push refuses
 * everyone).
 */
const roles = ["viewer", "developer", "maintainer", "admin", "owner"] as const;
type Role = (typeof roles)[number];
const rank: Record<Role, number> = {
  viewer: 0,
  developer: 1,
  maintainer: 2,
  admin: 3,
  owner: 4,
};

interface Action {
  id: string;
  label: string;
  permission: string;
  min: Role;
  /** Which protection, when on, applies to this action. */
  guard?: "direct" | "force";
}

const actions: Action[] = [
  {
    id: "read",
    label: "Clone and pull",
    permission: "tree:read",
    min: "viewer",
  },
  {
    id: "fork",
    label: "Fork into another tenant",
    permission: "tree:fork",
    min: "viewer",
  },
  {
    id: "push",
    label: "Push a feature branch",
    permission: "branch:push",
    min: "developer",
  },
  {
    id: "main",
    label: "Push straight to main",
    permission: "branch:push",
    min: "developer",
    guard: "direct",
  },
  {
    id: "mr",
    label: "Open a merge request",
    permission: "mr:create",
    min: "developer",
  },
  {
    id: "merge",
    label: "Merge a merge request",
    permission: "mr:merge",
    min: "maintainer",
  },
  {
    id: "delete",
    label: "Delete a branch",
    permission: "branch:delete",
    min: "maintainer",
  },
  {
    id: "force",
    label: "Force-push main",
    permission: "branch:force_push",
    min: "admin",
    guard: "force",
  },
  {
    id: "protect",
    label: "Change protections",
    permission: "tree:admin",
    min: "admin",
  },
  {
    id: "iam",
    label: "Manage teams and policies",
    permission: "iam:admin",
    min: "admin",
  },
];

interface Decision {
  allowed: boolean;
  reason: string;
  stage: "role" | "protection";
}

function decide(
  action: Action,
  role: Role,
  protect: { direct: boolean; force: boolean },
): Decision {
  if (rank[role] < rank[action.min]) {
    return {
      allowed: false,
      stage: "role",
      reason: `${action.permission} needs ${action.min} or higher; ${role} does not include it.`,
    };
  }
  if (action.guard === "force" && protect.force) {
    return {
      allowed: false,
      stage: "protection",
      reason:
        "main refuses force pushes (no_force_push), for every role including owners.",
    };
  }
  if (action.guard === "direct" && protect.direct) {
    return role === "owner"
      ? {
          allowed: true,
          stage: "protection",
          reason:
            "main takes changes through merge requests, but owners may push with --bypass-protection; the push is audited as a bypass.",
        }
      : {
          allowed: false,
          stage: "protection",
          reason:
            "main takes changes through merge requests only (no direct push).",
        };
  }
  return {
    allowed: true,
    stage: "role",
    reason: `${role} includes ${action.permission}.`,
  };
}

export function AccessDemo() {
  const [role, setRole] = useState<Role>("developer");
  const [selected, setSelected] = useState("main");
  const [protect, setProtect] = useState({ direct: true, force: true });
  const action = actions.find((item) => item.id === selected)!;
  const decision = decide(action, role, protect);

  return (
    <DiagramFrame
      label="Access on acme/platform · main"
      bodyClassName="p-0 sm:p-0"
      footer="Custom roles, per-tree grants and deny-overrides policies with attribute conditions refine this; the admin panel's simulator explains any real decision."
    >
      <div className="flex flex-col gap-4 border-b border-separator-border p-4 sm:p-5">
        <div className="flex flex-col gap-2">
          <span className="eyebrow">Role in the tenant</span>
          <div
            role="radiogroup"
            aria-label="Role"
            className="flex flex-wrap gap-1 rounded-xl bg-background-secondary-default p-1"
          >
            {roles.map((item) => (
              <button
                key={item}
                type="button"
                role="radio"
                aria-checked={item === role}
                onClick={() => setRole(item)}
                className={cx(
                  "relative isolate inline-flex h-8 flex-1 items-center justify-center rounded-lg px-2.5 text-body-2-medium capitalize transition-colors",
                  item === role
                    ? "text-text-primary"
                    : "text-text-secondary hover:text-text-primary",
                )}
              >
                {item === role ? (
                  <motion.span
                    layoutId="access-role"
                    className="absolute inset-0 -z-10 rounded-lg bg-surface-raised shadow-xs"
                    transition={{ type: "spring", stiffness: 450, damping: 38 }}
                  />
                ) : null}
                {item}
              </button>
            ))}
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Toggle
            on={protect.direct}
            label="No direct push to main"
            onClick={() => setProtect((p) => ({ ...p, direct: !p.direct }))}
          />
          <Toggle
            on={protect.force}
            label="No force push to main"
            onClick={() => setProtect((p) => ({ ...p, force: !p.force }))}
          />
        </div>
      </div>

      <div className="grid md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <ul className="flex flex-col border-b border-separator-border py-2 md:border-e md:border-b-0">
          {actions.map((item) => {
            const result = decide(item, role, protect);
            const active = item.id === selected;
            return (
              <li key={item.id}>
                <button
                  type="button"
                  onClick={() => setSelected(item.id)}
                  aria-pressed={active}
                  className={cx(
                    "flex w-full items-center gap-3 px-4 py-2 text-start transition-colors sm:px-5",
                    active
                      ? "bg-background-secondary-default"
                      : "hover:bg-background-secondary-default",
                  )}
                >
                  <span
                    className={cx(
                      "flex size-5 shrink-0 items-center justify-center rounded-full transition-colors",
                      result.allowed
                        ? "bg-text-primary text-background-full"
                        : "border border-dashed border-text-tertiary text-text-tertiary",
                    )}
                  >
                    {result.allowed ? (
                      <RiCheckLine className="size-3.5" aria-label="allowed" />
                    ) : (
                      <RiCloseLine className="size-3.5" aria-label="denied" />
                    )}
                  </span>
                  <span
                    className={cx(
                      "flex-1 text-body-2-regular",
                      result.allowed
                        ? "text-text-primary"
                        : "text-text-secondary",
                    )}
                  >
                    {item.label}
                  </span>
                  {item.guard ? (
                    <RiShieldCheckLine
                      className="size-3.5 text-foreground-icon-tertiary"
                      aria-label="protected action"
                    />
                  ) : null}
                </button>
              </li>
            );
          })}
        </ul>

        <div className="relative min-h-56 p-4 sm:p-5">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={`${selected}-${role}-${protect.direct}-${protect.force}`}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.3, ease }}
              className="flex flex-col gap-4"
            >
              <span
                className={cx(
                  "inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-0.5 text-caption-1-medium",
                  decision.allowed
                    ? "bg-text-primary text-background-full"
                    : "hatch border border-border-button-default text-text-primary",
                )}
              >
                {decision.allowed ? (
                  <RiCheckLine className="size-3.5" aria-hidden />
                ) : (
                  <RiCloseLine className="size-3.5" aria-hidden />
                )}
                {decision.allowed ? "Allowed" : "Denied"}
              </span>
              <div className="flex flex-col gap-1">
                <span className="text-title-3-semibold tracking-[-0.01em]">
                  {action.label}
                </span>
                <code className="font-mono text-caption-1-regular text-text-secondary">
                  {action.permission} on acme/platform
                  {action.guard ? " · main" : ""}
                </code>
              </div>
              <p className="text-body-regular leading-6 text-text-secondary">
                {decision.reason}
              </p>
              <ol className="mt-1 flex flex-col gap-1.5 text-caption-1-regular text-text-secondary">
                {[
                  "Role and grants",
                  "Policies (deny overrides)",
                  "Branch protection",
                ].map((stage, index) => {
                  const decisive =
                    (decision.stage === "role" && index === 0) ||
                    (decision.stage === "protection" && index === 2);
                  return (
                    <li key={stage} className="flex items-center gap-2">
                      <span className="font-mono text-caption-2-regular tabular-nums text-text-tertiary">
                        0{index + 1}
                      </span>
                      <span
                        className={decisive ? "text-text-primary" : undefined}
                      >
                        {stage}
                      </span>
                      {decisive ? (
                        <span
                          className="h-px flex-1 bg-text-primary"
                          aria-hidden
                        />
                      ) : null}
                    </li>
                  );
                })}
              </ol>
            </motion.div>
          </AnimatePresence>
        </div>
      </div>
    </DiagramFrame>
  );
}

function Toggle({
  on,
  label,
  onClick,
}: {
  on: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={onClick}
      className="inline-flex h-8 items-center gap-2 rounded-lg border border-border-button-default bg-background-primary-default ps-1.5 pe-2.5 text-body-2-regular text-text-primary shadow-xs transition-colors hover:border-border-button-hover"
    >
      <span
        className={cx(
          "relative inline-flex h-4.5 w-8 items-center rounded-full transition-colors",
          on ? "bg-text-primary" : "bg-background-tertiary-default",
        )}
      >
        <motion.span
          className="absolute size-3.5 rounded-full bg-background-full shadow-xs"
          animate={{ x: on ? 15 : 2 }}
          transition={{ type: "spring", stiffness: 500, damping: 35 }}
        />
      </span>
      {label}
    </button>
  );
}
