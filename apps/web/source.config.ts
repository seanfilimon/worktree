import {
  defineDocs,
  defineCollections,
  defineConfig,
  frontmatterSchema,
} from "fumadocs-mdx/config";
import { z } from "zod";

export const docs = defineDocs({
  dir: "content/docs",
});

export const guides = defineDocs({
  dir: "content/guides",
});

export const articles = defineCollections({
  type: "doc",
  dir: "content/articles",
  schema: frontmatterSchema.extend({
    author: z.string(),
    date: z.string().date().or(z.date()),
    image: z.string().optional(),
    tags: z.array(z.string()).optional(),
    summary: z.string().optional(),
  }),
});

export const maintainers = defineDocs({
  dir: "content/maintainers",
  docs: {
    schema: frontmatterSchema.extend({
      name: z.string().optional(),
      role: z.string().optional(),
      avatar: z.string().optional(),
      github: z.string().optional(),
      twitter: z.string().optional(),
      bio: z.string().optional(),
    }),
  },
});

export default defineConfig({
  mdxOptions: {
    // rehype/remark plugins can be added here
  },
});
