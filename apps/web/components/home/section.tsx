import type { ReactNode } from 'react';
import { RiArrowRightLine } from 'react-icons/ri';
import { TextLink } from '@/components/site/action';
import { Band, SectionHeading, gutter } from '@/components/site/frame';
import { StaggerItem, StaggerList } from '@/components/site/motion-text';
import { cx } from '@/utils/cx';
import { Reveal } from './motion';

export interface Point {
  lead: string;
  body: ReactNode;
}

/** Three or four short claims, each a bold lead-in and one sentence: easy to scan, hard to misread. */
export function Points({ points, className }: { points: Point[]; className?: string }) {
  return (
    <StaggerList className={cx('flex flex-col gap-1', className)} delay={0.15}>
      {points.map((point) => (
        <StaggerItem
          key={point.lead}
          className="group/point -mx-3 flex gap-3 rounded-xl px-3 py-1.5 text-body-regular leading-6 transition-colors duration-200 hover:bg-background-secondary-default"
        >
          {/* The rule stretches toward the claim under the pointer. */}
          <span
            aria-hidden
            className="mt-[0.7rem] h-px w-3 shrink-0 bg-text-primary transition-[width] duration-300 ease-out group-hover/point:w-6"
          />
          <span className="text-text-secondary transition-colors duration-200 group-hover/point:text-text-primary">
            <span className="text-body-medium text-text-primary">{point.lead}</span> {point.body}
          </span>
        </StaggerItem>
      ))}
    </StaggerList>
  );
}

interface ExplainerProps {
  id: string;
  index: string;
  eyebrow: string;
  title: ReactNode;
  lede: ReactNode;
  points: Point[];
  href: string;
  linkLabel: string;
  visual: ReactNode;
  /** A sunken surface inside the rails, to alternate the rhythm of consecutive chapters. */
  sunken?: boolean;
}

/** Text beside a diagram. `reverse` puts the diagram first on wide screens. */
export function SplitExplainer({
  reverse = false,
  ...props
}: ExplainerProps & { reverse?: boolean }) {
  return (
    <Band id={props.id} frameClassName={cx(props.sunken && 'bg-surface-sunken')}>
      <div
        className={cx('grid items-center gap-12 py-16 md:py-20 lg:grid-cols-12 lg:gap-10', gutter)}
      >
        <div
          className={cx(
            'flex flex-col gap-8 lg:col-span-5',
            reverse && 'lg:order-2 lg:col-start-8',
          )}
        >
          <SectionHeading index={props.index} eyebrow={props.eyebrow} title={props.title}>
            {props.lede}
          </SectionHeading>
          <Points points={props.points} />
          <TextLink href={props.href} trailingIcon={RiArrowRightLine}>
            {props.linkLabel}
          </TextLink>
        </div>
        <Reveal className={cx('min-w-0 lg:col-span-7', reverse ? 'lg:order-1' : 'lg:col-start-6')}>
          {props.visual}
        </Reveal>
      </div>
    </Band>
  );
}

/** Heading and claims side by side, with a full-width diagram below. */
export function StackedExplainer(props: ExplainerProps) {
  return (
    <Band id={props.id} frameClassName={cx(props.sunken && 'bg-surface-sunken')}>
      <div className={cx('py-16 md:py-20', gutter)}>
        <div className="grid gap-8 lg:grid-cols-12">
          <SectionHeading
            index={props.index}
            eyebrow={props.eyebrow}
            title={props.title}
            className="lg:col-span-6"
          >
            {props.lede}
          </SectionHeading>
          <div className="flex flex-col gap-5 lg:col-span-5 lg:col-start-8 lg:pt-10">
            <Points points={props.points} />
            <TextLink href={props.href} trailingIcon={RiArrowRightLine}>
              {props.linkLabel}
            </TextLink>
          </div>
        </div>
        <Reveal className="mt-12 min-w-0">{props.visual}</Reveal>
      </div>
    </Band>
  );
}
