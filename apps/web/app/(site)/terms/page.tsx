import type { Metadata } from "next";
import { PolicyNav } from "@/components/project/prose-aside";
import { PageHero, ProseBand } from "@/components/site/page-hero";

export const metadata: Metadata = {
  title: "Terms of Service",
  description: "Terms of Service for W0rkTree",
};

export default function TermsPage() {
  return (
    <>
      <PageHero eyebrow="Legal" title="Terms of Service" />
      <ProseBand aside={<PolicyNav current="/terms" />}>
        <p>This is a placeholder for the W0rkTree Terms of Service.</p>
      </ProseBand>
    </>
  );
}
