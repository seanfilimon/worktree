import defaultMdxComponents from "fumadocs-ui/mdx";
import { Accordion, Accordions } from "fumadocs-ui/components/accordion";
import { File, Files, Folder } from "fumadocs-ui/components/files";
import { ImageZoom } from "fumadocs-ui/components/image-zoom";
import { Step, Steps } from "fumadocs-ui/components/steps";
import { Tab, Tabs } from "fumadocs-ui/components/tabs";
import { TypeTable } from "fumadocs-ui/components/type-table";
import type { MDXComponents } from "mdx/types";
import type { ComponentProps } from "react";
import { Callout } from "@/components/docs/callout";
import { Card, Cards } from "@/components/docs/card";

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    // BoardUI-styled replacements for Fumadocs' defaults (components/docs), shared with better-iam.com.
    Callout,
    Card,
    Cards,
    img: (props: ComponentProps<"img">) => (
      <ImageZoom {...(props as ComponentProps<typeof ImageZoom>)} />
    ),
    Tabs,
    Tab,
    Steps,
    Step,
    Accordion,
    Accordions,
    Files,
    Folder,
    File,
    TypeTable,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
