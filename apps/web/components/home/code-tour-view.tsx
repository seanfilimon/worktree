'use client';

import {
  useEffect,
  useRef,
  useState,
  type FocusEvent,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from 'react';
import {
  AnimatePresence,
  animate,
  motion,
  stagger,
  useAnimate,
  useInView,
  useMotionValue,
  type Variants,
} from 'motion/react';
import { RiCheckLine, RiFileCodeLine, RiFileCopyLine } from 'react-icons/ri';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { cx } from '@/utils/cx';
import { PlayToggle } from './frame';
import { ease, useReducedMotionSafe } from './motion';

export interface TourStep {
  id: string;
  title: string;
  summary: string;
  file: string;
  code: ReactNode;
  /** The plain source, for the copy button. */
  raw: string;
}

/** How long each step stays up while the tour plays by itself. */
const INTERVAL = 7000;

const list: Variants = {
  hidden: {},
  shown: { transition: { staggerChildren: 0.06, delayChildren: 0.1 } },
};
const item: Variants = {
  hidden: { opacity: 0, y: 10 },
  shown: { opacity: 1, y: 0, transition: { duration: 0.45, ease } },
};

/**
 * A numbered walk from configuration to UI: pick a step on the left, read its code on the right. While it is on
 * screen the tour advances by itself, with a thin progress line on the current step; hovering the steps or the code,
 * or keyboard focus inside, holds the line where it is, and picking a step stops the autoplay (play resumes it).
 */
export function CodeTourView({ steps }: { steps: TourStep[] }) {
  const [active, setActive] = useState(0);
  const [autoplay, setAutoplay] = useState(true);
  const [hovering, setHovering] = useState(false);
  const [focused, setFocused] = useState(false);
  // The first code block renders as is; later ones cascade in line by line.
  const [ready, setReady] = useState(false);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const strip = useRef<HTMLDivElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const inView = useInView(root, { amount: 0.35 });
  const reduced = useReducedMotionSafe();
  const progress = useMotionValue(0);
  const current = steps[active]!;
  const playing = autoplay && !reduced;
  const running = playing && inView && !hovering && !focused;

  useEffect(() => setReady(true), []);

  // Continue from wherever the progress line stopped, so a hover only holds it rather than restarting it.
  useEffect(() => {
    if (!running) return;
    const controls = animate(progress, 1, {
      duration: ((1 - progress.get()) * INTERVAL) / 1000,
      ease: 'linear',
      onComplete: () => {
        progress.set(0);
        setActive((index) => (index + 1) % steps.length);
      },
    });
    return () => controls.stop();
  }, [running, active, progress, steps.length]);

  // On small screens the steps are a sideways strip: keep the current one in it without moving the page.
  useEffect(() => {
    const list = strip.current;
    const tab = tabs.current[active];
    if (!list || !tab || list.scrollWidth <= list.clientWidth) return;
    const inset = parseFloat(getComputedStyle(list).paddingInlineStart) || 0;
    list.scrollTo({ left: tab.offsetLeft - inset, behavior: reduced ? 'auto' : 'smooth' });
  }, [active, reduced]);

  // Reading holds the progress line: the pointer over the steps or the code, or keyboard focus anywhere inside.
  const holdOnHover = {
    onPointerEnter: (event: PointerEvent<HTMLElement>) => {
      if (event.pointerType === 'mouse') setHovering(true);
    },
    onPointerLeave: () => setHovering(false),
  };

  function pick(index: number) {
    setAutoplay(false);
    progress.set(0);
    setActive(index);
  }

  function onKeyDown(event: KeyboardEvent) {
    const delta =
      event.key === 'ArrowDown' || event.key === 'ArrowRight'
        ? 1
        : event.key === 'ArrowUp' || event.key === 'ArrowLeft'
          ? -1
          : 0;
    if (!delta) return;
    event.preventDefault();
    const next = (active + delta + steps.length) % steps.length;
    pick(next);
    tabs.current[next]?.focus();
  }

  function onBlur(event: FocusEvent<HTMLDivElement>) {
    if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false);
  }

  return (
    <div
      ref={root}
      onFocus={(event) => {
        if (event.target.matches(':focus-visible')) setFocused(true);
      }}
      onBlur={onBlur}
      className="grid gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:gap-8"
    >
      <motion.div
        ref={strip}
        role="tablist"
        aria-orientation="vertical"
        aria-label="Integration steps"
        onKeyDown={onKeyDown}
        {...holdOnHover}
        data-lenis-prevent-horizontal
        variants={list}
        initial="hidden"
        whileInView="shown"
        viewport={{ once: true, amount: 0.3 }}
        className="relative -mx-5 flex snap-x scroll-px-5 gap-2 overflow-x-auto px-5 pb-1 sm:-mx-8 sm:scroll-px-8 sm:px-8 lg:mx-0 lg:flex-col lg:gap-1 lg:overflow-visible lg:px-0 lg:pb-0"
      >
        {steps.map((step, index) => {
          const selected = index === active;
          return (
            <motion.button
              key={step.id}
              ref={(element) => {
                tabs.current[index] = element;
              }}
              variants={item}
              type="button"
              role="tab"
              id={`tour-tab-${step.id}`}
              aria-selected={selected}
              aria-controls="tour-panel"
              tabIndex={selected ? 0 : -1}
              onClick={() => pick(index)}
              className={cx(
                'group relative flex shrink-0 cursor-pointer snap-start items-start gap-3 rounded-xl px-3 py-2.5 text-start transition-[background-color,border-color,scale] duration-200 active:scale-[0.99] lg:px-4 lg:py-3.5',
                'max-lg:border',
                selected
                  ? 'max-lg:border-border-button-default max-lg:bg-surface-raised max-lg:shadow-xs'
                  : 'max-lg:border-separator-border max-lg:hover:border-border-button-hover hover:bg-background-secondary-default',
              )}
            >
              {selected ? (
                <motion.span
                  layoutId="tour-highlight"
                  aria-hidden
                  className="absolute inset-0 hidden rounded-xl border border-border-button-default bg-surface-raised shadow-xs lg:block"
                  transition={{ type: 'spring', stiffness: 420, damping: 36 }}
                />
              ) : null}
              <span
                className={cx(
                  'relative mt-px flex size-6 shrink-0 items-center justify-center overflow-hidden rounded-full border font-mono text-caption-2-medium tabular-nums transition-colors duration-200',
                  selected
                    ? 'border-text-primary bg-text-primary text-background-full'
                    : 'border-border-button-default bg-background-primary-default text-text-secondary group-hover:border-text-primary group-hover:text-text-primary',
                )}
              >
                {/* Hovering a step previews it: its number fills with ink from the bottom. */}
                {selected ? null : (
                  <span
                    aria-hidden
                    className="absolute inset-0 origin-bottom scale-y-0 bg-text-primary transition-transform duration-300 ease-out group-hover:scale-y-100"
                  />
                )}
                <span
                  className={cx(
                    'relative transition-colors duration-200',
                    !selected && 'group-hover:text-background-full',
                  )}
                >
                  {index + 1}
                </span>
              </span>
              <span className="relative flex min-w-0 flex-col gap-1">
                <span
                  className={cx(
                    'text-body-medium whitespace-nowrap transition-colors lg:whitespace-normal',
                    selected
                      ? 'text-text-primary'
                      : 'text-text-secondary group-hover:text-text-primary',
                  )}
                >
                  {step.title}
                </span>
                <span className="hidden text-body-2-regular text-text-secondary lg:block">
                  {step.summary}
                </span>
              </span>
              {selected && playing ? (
                <span
                  aria-hidden
                  className="absolute inset-x-3 bottom-1 h-0.5 overflow-hidden rounded-full bg-background-tertiary-default lg:inset-x-4 lg:bottom-1.5"
                >
                  <motion.span
                    className="absolute inset-0 origin-left rounded-full bg-text-primary"
                    style={{ scaleX: progress }}
                  />
                </span>
              ) : null}
            </motion.button>
          );
        })}
      </motion.div>

      <div
        id="tour-panel"
        role="tabpanel"
        aria-labelledby={`tour-tab-${current.id}`}
        className="relative min-w-0 overflow-hidden rounded-2xl border border-border-button-default bg-surface-raised shadow-sm transition-colors duration-300 hover:border-border-button-hover"
      >
        <div className="flex h-11 items-center gap-2 border-b border-separator-border ps-5 pe-3 text-caption-1-regular text-text-secondary">
          <RiFileCodeLine
            className="size-3.5 shrink-0 text-foreground-icon-secondary"
            aria-hidden
          />
          <AnimatePresence mode="wait" initial={false}>
            <motion.span
              key={current.file}
              className="truncate font-mono"
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.2 }}
            >
              {current.file}
            </motion.span>
          </AnimatePresence>
          <span className="ms-auto shrink-0 font-mono text-text-tertiary tabular-nums">
            {active + 1} / {steps.length}
          </span>
          <span className="flex shrink-0 items-center gap-0.5">
            <PlayToggle
              compact
              paused={!playing}
              onClick={() => {
                if (playing) setAutoplay(false);
                else {
                  progress.set(0);
                  setAutoplay(true);
                }
              }}
            />
            <CopyCode text={current.raw} />
          </span>
        </div>
        <p className="border-b border-separator-border px-5 py-3 text-body-2-regular text-text-secondary lg:hidden">
          {current.summary}
        </p>
        <div
          {...holdOnHover}
          data-lenis-prevent-horizontal
          className="home-code relative min-h-[26rem] overflow-x-auto px-5 py-5 font-mono text-[0.8125rem] leading-6"
        >
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={current.id}
              initial={{ opacity: 0, y: 10, filter: 'blur(4px)' }}
              animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
              exit={{ opacity: 0, y: -6, filter: 'blur(4px)' }}
              transition={{ duration: 0.28, ease }}
            >
              <CodeLines cascade={ready && !reduced}>{current.code}</CodeLines>
            </motion.div>
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
}

/** Highlighted code whose lines fade in one after another when it mounts. */
function CodeLines({ cascade, children }: { cascade: boolean; children: ReactNode }) {
  const [scope, run] = useAnimate<HTMLDivElement>();
  // Decided once, on mount: a block that was already on screen never replays.
  const [animateIn] = useState(cascade);
  useEffect(() => {
    if (!animateIn) return;
    const lines = scope.current.querySelectorAll('.line');
    if (lines.length === 0) return;
    const controls = run(
      lines,
      { opacity: [0, 1], filter: ['blur(2px)', 'blur(0px)'] },
      { duration: 0.35, delay: stagger(0.025), ease },
    );
    return () => controls.stop();
  }, [animateIn, run, scope]);
  return <div ref={scope}>{children}</div>;
}

function CopyCode({ text }: { text: string }) {
  const [copied, onClick] = useCopyButton(() => navigator.clipboard.writeText(text));
  return (
    <span className="flex items-center">
      <AnimatePresence initial={false}>
        {copied ? (
          <motion.span
            key="copied"
            role="status"
            className="pe-1 text-caption-1-medium text-text-primary"
            initial={{ opacity: 0, x: 6 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 6 }}
            transition={{ duration: 0.2, ease }}
          >
            Copied
          </motion.span>
        ) : null}
      </AnimatePresence>
      <button
        type="button"
        onClick={onClick}
        aria-label={copied ? 'Copied' : 'Copy the code'}
        className="inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-lg text-foreground-icon-secondary transition-[color,background-color,scale] duration-200 hover:bg-background-secondary-default hover:text-foreground-icon-primary active:scale-90"
      >
        <AnimatePresence mode="wait" initial={false}>
          {copied ? (
            <motion.span
              key="check"
              className="flex size-5 items-center justify-center rounded-full bg-text-primary text-background-full"
              initial={{ scale: 0, rotate: -45 }}
              animate={{ scale: 1, rotate: 0 }}
              exit={{ scale: 0, opacity: 0 }}
              transition={{ type: 'spring', stiffness: 520, damping: 22 }}
            >
              <RiCheckLine className="size-3.5" aria-hidden />
            </motion.span>
          ) : (
            <motion.span
              key="copy"
              className="flex"
              initial={{ scale: 0.6, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.6, opacity: 0 }}
              transition={{ duration: 0.15 }}
            >
              <RiFileCopyLine className="size-3.5" aria-hidden />
            </motion.span>
          )}
        </AnimatePresence>
      </button>
    </span>
  );
}
