import { RiArrowRightLine } from 'react-icons/ri';
import { Points, type Point } from '@/components/home/section';
import { Reveal } from '@/components/home/motion';
import { TextLink } from '@/components/site/action';
import { Band, SectionHeading, gutter } from '@/components/site/frame';
import { cx } from '@/utils/cx';
import { CommandFrame } from './command-frame';

export interface FeatureChapterProps {
  id: string;
  index: string;
  eyebrow: string;
  title: string;
  intro?: string;
  points: Point[];
  command?: string;
  learnMore: { label: string; href: string };
  /** Put the command card first on wide screens, to alternate consecutive chapters. */
  reverse?: boolean;
  sunken?: boolean;
}

/**
 * One feature group on the features page, as a chapter: numbered heading, the group's claims as points, a link to
 * the guide, and the group's example command in a diagram card. Groups without a command stack their points
 * beside the heading instead.
 */
export function FeatureChapter({
  id,
  index,
  eyebrow,
  title,
  intro,
  points,
  command,
  learnMore,
  reverse = false,
  sunken = false,
}: FeatureChapterProps) {
  const heading = (
    <SectionHeading index={index} eyebrow={eyebrow} title={title}>
      {intro}
    </SectionHeading>
  );
  const link = (
    <TextLink href={learnMore.href} trailingIcon={RiArrowRightLine}>
      {learnMore.label}
    </TextLink>
  );

  if (!command) {
    return (
      <Band id={id} frameClassName={cx(sunken && 'bg-surface-sunken')}>
        <div className={cx('grid gap-8 py-16 md:py-20 lg:grid-cols-12', gutter)}>
          <div className="lg:col-span-6">{heading}</div>
          <div className="flex flex-col gap-5 lg:col-span-5 lg:col-start-8 lg:pt-10">
            <Points points={points} />
            {link}
          </div>
        </div>
      </Band>
    );
  }

  return (
    <Band id={id} frameClassName={cx(sunken && 'bg-surface-sunken')}>
      <div className={cx('grid items-center gap-12 py-16 md:py-20 lg:grid-cols-12 lg:gap-10', gutter)}>
        <div
          className={cx(
            'flex flex-col gap-8 lg:col-span-6',
            reverse && 'lg:order-2 lg:col-start-7',
          )}
        >
          {heading}
          <Points points={points} />
          {link}
        </div>
        <Reveal
          className={cx(
            'min-w-0 lg:col-span-5',
            reverse ? 'lg:order-1 lg:col-start-1' : 'lg:col-start-8',
          )}
        >
          <CommandFrame command={command} />
        </Reveal>
      </div>
    </Band>
  );
}
