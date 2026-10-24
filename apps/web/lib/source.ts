import { docs, guides, articles, maintainers } from "collections/server";
import { loader } from "fumadocs-core/source";
import { toFumadocsSource } from "fumadocs-mdx/runtime/server";

export const docsSource = loader({
  baseUrl: "/docs",
  source: docs.toFumadocsSource(),
});

export const guidesSource = loader({
  baseUrl: "/guides",
  source: guides.toFumadocsSource(),
});

export const articlesSource = loader({
  baseUrl: "/articles",
  source: toFumadocsSource(articles, []),
});

export const maintainersSource = loader({
  baseUrl: "/maintainers",
  source: maintainers.toFumadocsSource(),
});
