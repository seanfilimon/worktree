import {
  RiArrowRightUpLine,
  RiFeedbackLine,
  RiFileCodeLine,
} from "react-icons/ri";
import { newIssueUrl, sourceFileUrl } from "@/lib/shared";

/** Under the table of contents: the page's source and a prefilled issue. */
export function TocFooter({ file, url }: { file: string; url: string }) {
  const link =
    "inline-flex items-center gap-1.5 text-caption-1-regular text-text-secondary transition-colors hover:text-text-primary";
  return (
    <div className="mt-4 flex flex-col gap-2 border-t border-separator-border pt-4">
      <a
        href={sourceFileUrl(file)}
        target="_blank"
        rel="noreferrer noopener"
        className={link}
      >
        <RiFileCodeLine className="size-3.5" aria-hidden /> Edit this page{" "}
        <RiArrowRightUpLine className="size-3" aria-hidden />
      </a>
      <a
        href={newIssueUrl(`Docs: ${url}`, `Page: ${url}\n\n`)}
        target="_blank"
        rel="noreferrer noopener"
        className={link}
      >
        <RiFeedbackLine className="size-3.5" aria-hidden /> Report an issue{" "}
        <RiArrowRightUpLine className="size-3" aria-hidden />
      </a>
    </div>
  );
}
