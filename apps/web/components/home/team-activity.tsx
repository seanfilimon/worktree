"use client";

import { AnimatePresence, motion } from "motion/react";
import { cx } from "@/utils/cx";
import { DiagramFrame, PlayToggle, ReplayButton } from "./frame";
import { ease, useStepper } from "./motion";

/**
 * Staged visibility as it happens: server events on the left (the live event stream), the team's work in
 * progress as `wt status --team` shows it on the right. A push ends the pusher's default staged slot, and
 * overlapping paths are called out the way `wt push` notes them.
 */
interface Staged {
  who: string;
  branch: string;
  label: string;
  path: string;
}

const events: {
  type: string;
  who: string;
  text: string;
  add?: Staged;
  end?: string;
}[] = [
  {
    type: "snapshot.staged",
    who: "alice",
    text: "staged services/api/src/auth.rs on feature/auth",
    add: {
      who: "alice",
      branch: "feature/auth",
      label: "default",
      path: "services/api/src/auth.rs",
    },
  },
  {
    type: "snapshot.staged",
    who: "bob",
    text: 'staged apps/web/app/login.tsx on main as "spike"',
    add: {
      who: "bob",
      branch: "main",
      label: "spike",
      path: "apps/web/app/login.tsx",
    },
  },
  {
    type: "snapshot.staged",
    who: "carol",
    text: "staged services/api/src/auth.rs on feature/auth",
    add: {
      who: "carol",
      branch: "feature/auth",
      label: "default",
      path: "services/api/src/auth.rs",
    },
  },
  {
    type: "branch.pushed",
    who: "alice",
    text: "pushed feature/auth; her staged slot ends",
    end: "alice",
  },
  {
    type: "staged.cleared",
    who: "carol",
    text: "rebased on alice's push and ran wt unstage",
    end: "carol",
  },
];

export function TeamActivity() {
  const tour = useStepper(events.length, { interval: 2400, hold: 4200 });
  const shown = events.slice(0, tour.step + 1);
  const staged: Staged[] = [];
  for (const event of shown) {
    if (event.add) staged.push(event.add);
    if (event.end) {
      const index = staged.findIndex((row) => row.who === event.end);
      if (index >= 0) staged.splice(index, 1);
    }
  }
  const overlap = staged
    .map((row) => row.path)
    .find((path, index, all) => all.indexOf(path) !== index);

  return (
    <div
      ref={tour.ref}
      onPointerEnter={() => tour.setHovered(true)}
      onPointerLeave={() => tour.setHovered(false)}
    >
      <DiagramFrame
        label="acme/platform · live"
        live
        bodyClassName="p-0 sm:p-0"
        actions={
          <>
            <ReplayButton onClick={tour.replay} />
            <PlayToggle paused={tour.paused} onClick={tour.toggle} compact />
          </>
        }
        footer="Staged snapshots are visible, never locks: pushing prints the overlap as a note and goes ahead."
      >
        <div className="grid md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
          <ol className="flex flex-col gap-2 border-b border-separator-border p-4 sm:p-5 md:border-e md:border-b-0">
            <li className="eyebrow mb-1">GET /api/v1/events</li>
            <AnimatePresence initial={false}>
              {shown.map((event, index) => (
                <motion.li
                  key={index}
                  layout
                  initial={{ opacity: 0, x: -10 }}
                  animate={{ opacity: index === tour.step ? 1 : 0.55, x: 0 }}
                  transition={{ duration: 0.4, ease }}
                  className="flex flex-col gap-0.5 rounded-lg border border-separator-border bg-background-full px-3 py-2"
                >
                  <span className="font-mono text-caption-2-regular text-text-tertiary">
                    {event.type}
                  </span>
                  <span className="text-caption-1-regular leading-5 text-text-secondary">
                    <span className="text-text-primary">{event.who}</span>{" "}
                    {event.text}
                  </span>
                </motion.li>
              ))}
            </AnimatePresence>
          </ol>
          <div className="flex min-w-0 flex-col">
            <div className="eyebrow px-4 pt-4 sm:px-5 sm:pt-5">
              wt status --team
            </div>
            <ul className="flex flex-col gap-1.5 p-4 font-mono text-[0.78rem] sm:p-5">
              <AnimatePresence initial={false}>
                {staged.map((row) => (
                  <motion.li
                    key={row.who}
                    layout
                    initial={{ opacity: 0, y: 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, x: 12, transition: { duration: 0.3 } }}
                    transition={{ duration: 0.35, ease }}
                    className={cx(
                      "grid grid-cols-[3.5rem_minmax(0,1fr)] gap-x-3 gap-y-0.5 rounded-lg border px-3 py-2",
                      row.path === overlap
                        ? "hatch border-text-primary"
                        : "border-separator-border bg-background-full",
                    )}
                  >
                    <span className="text-text-primary">{row.who}</span>
                    <span className="truncate text-text-secondary">
                      {row.branch}
                      {row.label !== "default" ? ` (${row.label})` : ""}
                    </span>
                    <span />
                    <span className="truncate text-text-primary">
                      {row.path}
                    </span>
                  </motion.li>
                ))}
              </AnimatePresence>
              {staged.length === 0 ? (
                <li className="px-3 py-2 text-text-tertiary">
                  no staged work in progress
                </li>
              ) : null}
            </ul>
            <AnimatePresence>
              {overlap ? (
                <motion.p
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  className="mx-4 mb-4 rounded-lg border border-dashed border-text-tertiary px-3 py-2 font-mono text-[0.78rem] text-text-primary sm:mx-5 sm:mb-5"
                >
                  note:{" "}
                  {staged
                    .filter((row) => row.path === overlap)
                    .map((row) => row.who)
                    .join(" and ")}{" "}
                  have staged changes to {overlap}
                </motion.p>
              ) : null}
            </AnimatePresence>
          </div>
        </div>
      </DiagramFrame>
    </div>
  );
}
