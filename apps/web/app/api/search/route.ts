import { docsSource, guidesSource } from "@/lib/source";
import { createSearchAPI } from "fumadocs-core/search/server";

export const revalidate = false;

// One index over the reference docs and the guides, so the docs search box
// also finds tutorials such as the quick start and migration guide.
const pages = [...docsSource.getPages(), ...guidesSource.getPages()];

export const { GET } = createSearchAPI("advanced", {
  indexes: pages.map((page) => ({
    id: page.url,
    url: page.url,
    title: page.data.title,
    description: page.data.description,
    structuredData: page.data.structuredData,
  })),
});
