"use client";

import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cx } from "@/utils/cx";
import { PlayToggle } from "./frame";
import { ease, useStepper } from "./motion";

export interface Scene {
  title: string;
  summary: string;
  /** The terminal session, highlighted on the server. */
  code: ReactNode;
}

/**
 * The hero's three-scene strip and terminal: numbered tabs above one window, advancing on their own while the
 * hero is on screen (paused while pointed at, and pausable). The construction of better-iam.com's hero window.
 */
export function HeroScenes({ scenes }: { scenes: Scene[] }) {
  const tour = useStepper(scenes.length, { interval: 6500, hold: 6500 });
  const scene = scenes[tour.step]!;
  return (
    <div
      ref={tour.ref}
      onPointerEnter={() => tour.setHovered(true)}
      onPointerLeave={() => tour.setHovered(false)}
      className="overflow-hidden rounded-2xl border border-border-button-default bg-surface-raised shadow-sm"
    >
      <div
        role="tablist"
        aria-label="What W0rkTree does"
        className="grid grid-cols-1 gap-px border-b border-separator-border bg-separator-border sm:grid-cols-3"
      >
        {scenes.map((item, index) => {
          const active = index === tour.step;
          return (
            <button
              key={item.title}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => tour.select(index)}
              className={cx(
                "relative flex flex-col gap-1 bg-surface-raised px-4 py-3 text-start transition-colors sm:px-5",
                active
                  ? "text-text-primary"
                  : "text-text-secondary hover:text-text-primary",
              )}
            >
              <span className="flex items-center gap-2.5">
                <span
                  className={cx(
                    "inline-flex size-6 items-center justify-center rounded-full border font-mono text-caption-2-regular tabular-nums transition-colors",
                    active
                      ? "border-text-primary bg-text-primary text-background-full"
                      : "border-border-button-default text-text-tertiary",
                  )}
                >
                  {String(index + 1).padStart(2, "0")}
                </span>
                <span className="text-body-medium">{item.title}</span>
              </span>
              <span className="hidden text-caption-1-regular leading-5 text-text-secondary sm:block">
                {item.summary}
              </span>
              {active ? (
                <motion.span
                  layoutId="hero-scene"
                  aria-hidden
                  className="absolute inset-x-0 -bottom-px h-px bg-text-primary"
                  transition={{ type: "spring", stiffness: 450, damping: 40 }}
                />
              ) : null}
            </button>
          );
        })}
      </div>
      <div className="flex h-10 items-center gap-2 border-b border-separator-border px-4 text-caption-1-regular text-text-secondary sm:px-5">
        <span aria-hidden className="flex gap-1.5">
          <span className="size-2.5 rounded-full border border-border-button-default" />
          <span className="size-2.5 rounded-full border border-border-button-default" />
          <span className="size-2.5 rounded-full border border-border-button-default" />
        </span>
        <span className="ms-2 font-mono">~/acme</span>
        <span className="ms-auto flex items-center gap-2">
          <span className="font-mono tabular-nums text-text-tertiary">
            {String(tour.step + 1).padStart(2, "0")} /{" "}
            {String(scenes.length).padStart(2, "0")}
          </span>
          <PlayToggle paused={tour.paused} onClick={tour.toggle} compact />
        </span>
      </div>
      <div className="home-code relative min-h-[17rem] overflow-x-auto px-4 py-5 font-mono text-[0.8125rem] leading-6 sm:px-6">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={tour.step}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.35, ease }}
          >
            {scene.code}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}
