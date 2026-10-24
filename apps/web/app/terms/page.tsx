import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Terms of Service - W0rkTree",
  description: "Terms of Service for W0rkTree",
};

export default function TermsPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16">
      <section className="mb-16">
        <h1 className="mb-4 text-4xl font-bold tracking-tight sm:text-5xl">
          Terms of Service
        </h1>
        <p className="text-muted-foreground">
          This is a placeholder for the W0rkTree Terms of Service.
        </p>
      </section>
    </div>
  );
}
