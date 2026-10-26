import Link from "next/link";
import { cx } from "@/utils/cx";

/** The policy pages, listed beside each one so a reader can move between them. */
const policies: { label: string; href: string }[] = [
  { label: "Code of Conduct", href: "/code-of-conduct" },
  { label: "Security", href: "/security" },
  { label: "Privacy Policy", href: "/privacy" },
  { label: "Terms of Service", href: "/terms" },
];

const asideLink =
  "block border-s py-1 ps-3 text-body-2-regular transition-colors duration-200 hover:text-text-primary";

/** "On this page": anchors to a ProseBand's sections, for its sticky aside. */
export function OnThisPage({ items }: { items: { id: string; label: string }[] }) {
  return (
    <nav aria-label="On this page" className="flex flex-col gap-3">
      <p className="eyebrow">On this page</p>
      <ul className="flex flex-col">
        {items.map((item) => (
          <li key={item.id}>
            <a
              href={`#${item.id}`}
              className={cx(asideLink, "border-separator-border text-text-secondary hover:border-text-primary")}
            >
              {item.label}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/** The other policy pages, with the current one marked. */
export function PolicyNav({ current }: { current: string }) {
  return (
    <nav aria-label="Policies" className="flex flex-col gap-3">
      <p className="eyebrow">Policies</p>
      <ul className="flex flex-col">
        {policies.map((policy) => {
          const active = policy.href === current;
          return (
            <li key={policy.href}>
              <Link
                href={policy.href}
                aria-current={active ? "page" : undefined}
                className={cx(
                  asideLink,
                  active
                    ? "border-text-primary text-text-primary"
                    : "border-separator-border text-text-secondary hover:border-text-primary",
                )}
              >
                {policy.label}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
