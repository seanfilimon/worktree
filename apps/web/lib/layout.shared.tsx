import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: "W0rkTree",
    },
    links: [
      {
        text: "Docs",
        url: "/docs",
        active: "nested-url",
      },
      {
        text: "Guides",
        url: "/guides",
        active: "nested-url",
      },
      {
        text: "Articles",
        url: "/articles",
        active: "nested-url",
      },
      {
        text: "Maintainers",
        url: "/maintainers",
        active: "nested-url",
      },
    ],
  };
}
