import { ArrowLeft, Compass } from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";

export function NotFoundPage() {
  const { pathname } = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <PageHeader title="Page not found" breadcrumbs={[{ label: "Not found" }]} />
      <section
        aria-labelledby="not-found-title"
        className="flex flex-col items-center gap-4 rounded-lg border border-dashed px-6 py-16 text-center"
      >
        <span className="flex size-12 items-center justify-center rounded-full border">
          <Compass aria-hidden="true" className="size-6" />
        </span>
        <div className="space-y-1">
          <p className="font-mono text-xs text-muted-foreground">404 · {pathname}</p>
          <h2 id="not-found-title" className="text-base font-semibold">
            There is nothing here
          </h2>
          <p className="max-w-sm text-sm text-muted-foreground">
            The page may have moved, or the link is incorrect. Press <Kbd>/</Kbd> to search.
          </p>
        </div>
        <div className="flex flex-wrap justify-center gap-2">
          <Button variant="outline" onClick={() => void navigate(-1)}>
            <ArrowLeft aria-hidden="true" />
            Go back
          </Button>
          <Button asChild>
            <Link to="/">Go to dashboard</Link>
          </Button>
        </div>
      </section>
    </>
  );
}
