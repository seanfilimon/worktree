import { PageHeader } from "@/components/PageHeader";
import { ButtonLink } from "@/components/legacy/Button";
import { EmptyState } from "@/components/legacy/States";

export function NotFoundPage() {
  return (
    <>
      <PageHeader title="Page not found" />
      <EmptyState title="There is nothing here" action={<ButtonLink to="/">Go to dashboard</ButtonLink>}>
        The page may have moved, or the link is incorrect.
      </EmptyState>
    </>
  );
}
