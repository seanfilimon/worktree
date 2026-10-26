import { AttributesCard } from "@/components/abac/AttributesEditor";

/** v0.8 ABAC (§16.5): tenant attributes, matched by policy conditions as `tenant.<key>`. */
export function TenantAttributesCard({ slug, editable }: { slug: string; editable: boolean }) {
  return (
    <AttributesCard
      target={{ kind: "tenant", tenant: slug }}
      editable={editable}
      label="Tenant attributes"
      description={
        <>
          Key/value pairs that policy conditions match as <span className="font-mono">tenant.&lt;key&gt;</span>. Saving replaces the whole
          set.
        </>
      }
    />
  );
}
