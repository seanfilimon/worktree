import type { Metadata } from "next";
import { PolicyNav } from "@/components/project/prose-aside";
import { PageHero, ProseBand } from "@/components/site/page-hero";

export const metadata: Metadata = {
  title: "Privacy Policy",
  description: "Privacy Policy for W0rkTree",
};

export default function PrivacyPage() {
  return (
    <>
      <PageHero eyebrow="Legal" title="Privacy Policy" />
      <ProseBand aside={<PolicyNav current="/privacy" />}>
        <p>This is a placeholder for the W0rkTree Privacy Policy.</p>
      </ProseBand>
    </>
  );
}
