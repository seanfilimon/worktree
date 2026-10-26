/** Formatting helpers shared by the guides, articles, and maintainers pages. */

/** "January 15, 2025". Frontmatter dates are calendar days, so they are read and printed in UTC. */
export function formatDate(date: string | Date): string {
  return new Date(date).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
}

/** "2025-01-15", for `dateTime` attributes and the mono date lines on cards. */
export function isoDate(date: string | Date): string {
  return new Date(date).toISOString().slice(0, 10);
}

/** Two-digit index for card and section numbers: 1 becomes "01". */
export function pad(index: number): string {
  return String(index).padStart(2, "0");
}

/** The text Fumadocs extracts from a page for search, which is enough to estimate reading time. */
type Structured = {
  headings: { content: string }[];
  contents: { content: string }[];
};

/** Whole minutes at 220 words a minute, never less than one. */
export function readingMinutes(data: Structured | undefined): number {
  if (!data) return 1;
  const text = [...data.headings, ...data.contents].map((part) => part.content).join(" ");
  const words = text.split(/\s+/).filter(Boolean).length;
  return Math.max(1, Math.round(words / 220));
}

/** Initials for an avatar without a photo: "Sean Filimon" becomes "SF". */
export function initials(name: string): string {
  return name
    .split(" ")
    .filter(Boolean)
    .map((part) => part[0])
    .join("")
    .toUpperCase()
    .slice(0, 2);
}
