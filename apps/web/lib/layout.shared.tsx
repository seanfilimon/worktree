import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { DocsSearchFull, DocsSearchSm } from "@/components/docs-slots";
import { Logo } from "@/components/logo";
import { repositoryUrl } from "./shared";

/** Docs chrome shared with better-iam.com: the logo, nav links, BoardUI search triggers and the GitHub link. */
export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: <Logo />,
      transparentMode: "top",
    },
    githubUrl: repositoryUrl,
    // The theme toggle lives in the sidebar footer (components/sidebar-footer.tsx).
    slots: {
      searchTrigger: { full: DocsSearchFull, sm: DocsSearchSm },
      themeSwitch: false,
    },
    links: [
      { text: "Docs", url: "/docs", active: "nested-url", on: "nav" },
      { text: "Guides", url: "/guides", active: "nested-url", on: "nav" },
      { text: "Features", url: "/features", active: "url", on: "nav" },
      { text: "Changelog", url: "/changelog", active: "url", on: "nav" },
    ],
  };
}
