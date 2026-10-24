import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Privacy Policy - W0rkTree",
  description: "Privacy Policy for W0rkTree",
};

export default function PrivacyPage() {
  return (
    <div className="container mx-auto max-w-4xl px-6 py-16">
      <section className="mb-16">
        <h1 className="mb-4 text-4xl font-bold tracking-tight sm:text-5xl">
          Privacy Policy
        </h1>
        <p className="text-muted-foreground">
          This is a placeholder for the W0rkTree Privacy Policy.
        </p>
      </section>
    </div>
  );
}
