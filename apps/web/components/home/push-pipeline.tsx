'use client';

import type { KeyboardEvent, ReactNode } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { RiCheckLine, RiFileCodeLine, RiPauseLine, RiPlayLine } from 'react-icons/ri';
import { Chip } from '@/components/base/badges/chip';
import { cx } from '@/utils/cx';
import { ease, useStepper } from './motion';

/**
 * One push, from snapshot to fan-out, as one compact frame that plays by itself: a trace whose segments are sized
 * like each stage's share of the work, and a track of stage panels that slides to the stage in progress. Readers
 * can pick a stage (which pauses autoplay) or pause it outright. Adapted from better-iam.com's request pipeline;
 * the stages follow the client (crates/worktree-sdk remote/push.rs) and the server's push checks in order
 * (services/server-go internal/server/server.go).
 */
const INTERVAL = 4200;

const stages: {
  label: string;
  title: string;
  body: ReactNode;
  detail: ReactNode;
  note?: string;
  bar: [number, number];
  afterCommit?: boolean;
}[] = [
  {
    label: "Snapshot",
    title: "Record the working tree",
    detail: (
      <>
        Changed files hashed with <code>BLAKE3</code>, scanned, written to <code>.wt/objects</code>.
      </>
    ),
    body: "wt snapshot hashes every changed file, scans added and modified files for secrets, and writes the blobs, a manifest and the snapshot record to the local store. Nothing touches the network.",
    note: "secret found · snapshot refused",
    bar: [0, 14],
  },
  {
    label: "Negotiate",
    title: "Ask what the server lacks",
    detail: "Only objects missing on the server are sent; identical content anywhere is never sent twice.",
    body: "wt push lists the objects of every snapshot the server does not have yet and asks which of them are missing. Content already stored for any tree, in any tenant, is reused instead of uploaded.",
    bar: [14, 28],
  },
  {
    label: "Pack",
    title: "Upload one pack with deltas",
    detail: "A small edit to a large file travels as a few hundred bytes, not the whole file.",
    body: "The missing objects stream as one zstd-compressed pack, with binary deltas against objects the server already has. An interrupted upload resumes from the last byte the server kept; servers without packs get whole objects.",
    bar: [28, 50],
  },
  {
    label: "Check",
    title: "Authorize and run every check",
    detail: (
      <>
        <code>tree:write</code> and <code>branch:push</code>, then path rules, protection, quotas, config, secrets,
        signatures.
      </>
    ),
    body: "The server checks the caller's roles, grants and policies, then runs the push checkers in order: path ACLs, branch protection, quotas, the .wt-tree configuration, server-side secret scanning and signature verification. Any failure rejects the push with its reason.",
    note: "rejected · PermissionDenied",
    bar: [50, 78],
  },
  {
    label: "Advance",
    title: "Move the branch",
    detail: "The tip moves from the one your client saw, or the push is refused.",
    body: "The branch advances by compare-and-swap from the tip the client last fetched, so a concurrent push is never overwritten. A force push needs branch:force_push and a lease; the replaced tip stays in the server reflog.",
    note: "ConflictDetected · fetch and integrate",
    bar: [78, 88],
  },
  {
    label: "Fan-out",
    title: "Tell everyone who cares",
    detail: "branch.pushed: audit log, live events, webhooks, mirrors, notifications.",
    body: "After the branch moves, the push is audited and published: subscribers on the event stream, webhooks, Git mirrors and notifications react, and teammates' daemons fetch the new snapshots on their own.",
    bar: [88, 100],
    afterCommit: true,
  },
];

export function PushPipeline({ code }: { code: ReactNode }) {
  const { ref, step, select, toggle, paused, hovered, setHovered } = useStepper(stages.length, {
    interval: INTERVAL,
    hold: INTERVAL + 1800,
  });
  const complete = step === stages.length - 1;

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const delta = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (!delta) return;
    event.preventDefault();
    const next = (step + delta + stages.length) % stages.length;
    select(next);
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }

  return (
    <div
      ref={ref}
      className="border-t border-separator-border"
      onPointerEnter={(event) => event.pointerType === 'mouse' && setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      {/* Trace timeline */}
      <div className="px-5 pt-6 pb-5 sm:px-8 lg:px-12">
        <div className="mb-7 flex items-center gap-3">
          <span className="eyebrow">Trace · one push</span>
          <AnimatePresence mode="wait" initial={false}>
            {complete ? (
              <motion.span
                key="done"
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.3, ease }}
                className="inline-flex items-center gap-1.5 rounded-full bg-text-primary px-2.5 py-0.5 text-caption-1-medium text-background-full"
              >
                <RiCheckLine className="size-3.5" aria-hidden /> Accepted · pushed
              </motion.span>
            ) : (
              <motion.span
                key={paused ? 'paused' : hovered ? 'held' : 'running'}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="inline-flex items-center gap-1.5 rounded-full bg-background-secondary-default px-2.5 py-0.5 text-caption-1-medium text-text-secondary"
              >
                {paused ? (
                  'Paused'
                ) : hovered ? (
                  'Held while you read'
                ) : (
                  <>
                    <span className="live-dot" /> Running
                  </>
                )}
              </motion.span>
            )}
          </AnimatePresence>
          <button
            type="button"
            onClick={toggle}
            aria-label={paused ? 'Play the push' : 'Pause the push'}
            className="ms-auto inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-caption-1-medium text-text-secondary transition-colors hover:bg-background-secondary-default hover:text-text-primary"
          >
            {paused ? <RiPlayLine className="size-3.5" /> : <RiPauseLine className="size-3.5" />}
            {paused ? 'Play' : 'Pause'}
          </button>
        </div>

        <div
          role="tablist"
          aria-label="Push stages"
          onKeyDown={onKeyDown}
          className="relative flex gap-1.5"
        >
          {stages.map((stage, index) => {
            const state = index < step ? 'done' : index === step ? 'active' : 'pending';
            return (
              <button
                key={stage.label}
                type="button"
                role="tab"
                aria-selected={index === step}
                aria-controls="push-panel"
                tabIndex={index === step ? 0 : -1}
                onClick={() => select(index)}
                style={{ ['--w' as string]: stage.bar[1] - stage.bar[0] }}
                className="group flex min-w-0 flex-1 cursor-pointer flex-col gap-2 text-start sm:grow-[var(--w)]"
              >
                <span className="relative block h-2 overflow-hidden rounded-full bg-background-tertiary-default transition-[height,margin] duration-200 group-hover:-my-0.5 group-hover:h-3">
                  <motion.span
                    key={state === 'active' ? `${step}-${paused}-${hovered}` : state}
                    className={cx(
                      'absolute inset-0 origin-left rounded-full',
                      stage.afterCommit ? 'trace-bar-after' : 'bg-text-primary',
                    )}
                    initial={{
                      scaleX: state === 'done' || (state === 'active' && hovered) ? 1 : 0,
                    }}
                    animate={{ scaleX: state === 'pending' ? 0 : 1 }}
                    transition={
                      state === 'active' && !paused && !hovered
                        ? { duration: INTERVAL / 1000, ease: 'linear' }
                        : { duration: 0.3, ease }
                    }
                  />
                  {state === 'pending' ? (
                    <span className="absolute inset-0 origin-left scale-x-0 rounded-full bg-text-tertiary transition-transform duration-300 group-hover:scale-x-100" />
                  ) : null}
                </span>
                <span className="flex items-baseline gap-1.5">
                  <span
                    className={cx(
                      'font-mono text-caption-2-regular tabular-nums',
                      state === 'pending' ? 'text-text-tertiary' : 'text-text-primary',
                    )}
                  >
                    0{index + 1}
                  </span>
                  <span
                    className={cx(
                      'hidden truncate text-caption-1-medium transition-colors md:inline',
                      state === 'active'
                        ? 'text-text-primary'
                        : 'text-text-secondary group-hover:text-text-primary',
                    )}
                  >
                    {stage.label}
                  </span>
                </span>
              </button>
            );
          })}
          {/* The branch moves here: everything to the right of it happens after the push is accepted. */}
          <span
            aria-hidden
            className="pointer-events-none absolute -top-3 hidden h-8 border-s border-dashed border-text-tertiary sm:block"
            // Segments grow with their share of the request; five 6px gaps shift the 88% boundary by 6.6px.
            style={{ left: 'calc(88% + 6.6px)' }}
          >
            <span className="absolute -top-4 -translate-x-1/2 font-mono text-caption-2-regular whitespace-nowrap text-text-tertiary">
              tip moves
            </span>
          </span>
        </div>
      </div>

      {/* Code beside the sliding stage panels */}
      <div className="grid border-t border-separator-border lg:grid-cols-12">
        <div className="flex min-w-0 flex-col border-b border-separator-border lg:col-span-5 lg:border-e lg:border-b-0">
          <div className="flex h-10 items-center gap-2 border-b border-separator-border px-5 text-caption-1-regular text-text-secondary sm:px-8 lg:ps-12 lg:pe-5">
            <RiFileCodeLine className="size-3.5" aria-hidden />
            <span className="font-mono">terminal</span>
          </div>
          <div
            data-lenis-prevent-horizontal
            data-focus={step === 0 ? 'snapshot' : step === 4 ? 'force' : 'push'}
            className="pipeline-code home-code overflow-x-auto px-5 py-5 font-mono text-[0.78rem] leading-[1.35rem] sm:px-8 lg:ps-12 lg:pe-5"
          >
            {code}
          </div>
          <p className="mt-auto border-t border-separator-border px-5 py-3 text-caption-1-regular leading-5 text-text-secondary sm:px-8 lg:ps-12 lg:pe-5">
            Pushes from the CLI, the daemon&rsquo;s offline queue and server-side merges all run these checks.
          </p>
        </div>

        <div
          id="push-panel"
          role="tabpanel"
          aria-label={stages[step]!.title}
          className="relative min-w-0 overflow-hidden py-6 lg:col-span-7 lg:py-8"
        >
          <div className="ps-5 sm:ps-8">
            <motion.ol
              className="flex"
              initial={false}
              animate={{ x: `-${step * 84}%` }}
              transition={{ duration: 0.7, ease }}
            >
              {stages.map((stage, index) => {
                const current = index === step;
                return (
                  <li key={stage.label} className="w-[84%] shrink-0 pe-4" aria-hidden={!current}>
                    <motion.button
                      type="button"
                      tabIndex={-1}
                      onClick={() => select(index)}
                      className={cx(
                        'flex h-full w-full flex-col gap-3 rounded-2xl border p-5 text-start transition-[border-color,box-shadow] duration-300 sm:p-6',
                        current
                          ? 'cursor-default border-border-button-default bg-surface-raised shadow-sm hover:border-border-button-hover hover:shadow-md'
                          : 'cursor-pointer border-separator-border bg-transparent hover:border-border-button-hover',
                      )}
                      animate={{ opacity: current ? 1 : 0.4, scale: current ? 1 : 0.97 }}
                      // The next stage peeks in from the right; pointing at it brings it forward.
                      whileHover={current ? { y: -3 } : { opacity: 0.75, scale: 0.985 }}
                      whileTap={current ? undefined : { scale: 0.97 }}
                      transition={{ duration: 0.5, ease }}
                    >
                      <span className="flex items-center gap-2">
                        <span className="flex size-6 items-center justify-center rounded-full bg-text-primary font-mono text-caption-2-semibold text-background-full tabular-nums">
                          {index + 1}
                        </span>
                        <span className="eyebrow">{stage.label}</span>
                        {stage.afterCommit ? (
                          <Chip variant="caption" color="soft" className="ms-auto">
                            after the push
                          </Chip>
                        ) : null}
                      </span>
                      <span className="text-title-2-semibold tracking-[-0.015em]">
                        {stage.title}
                      </span>
                      <motion.span
                        key={current ? `detail-${step}` : 'detail'}
                        className="rounded-lg border border-dashed border-border-button-default px-3 py-2 text-body-2-regular text-text-primary [&_code]:font-mono [&_code]:text-caption-1-regular"
                        initial={current ? { opacity: 0, x: 12 } : false}
                        animate={{ opacity: 1, x: 0 }}
                        transition={{ duration: 0.5, ease, delay: 0.25 }}
                      >
                        {stage.detail}
                      </motion.span>
                      <span className="text-body-regular leading-6 text-text-secondary">
                        {stage.body}
                      </span>
                      {stage.note ? (
                        <span className="mt-auto w-fit rounded-md border border-border-button-default bg-background-secondary-default px-2 py-0.5 font-mono text-caption-2-regular text-text-secondary">
                          on failure: {stage.note}
                        </span>
                      ) : null}
                    </motion.button>
                  </li>
                );
              })}
            </motion.ol>
          </div>
        </div>
      </div>
    </div>
  );
}
