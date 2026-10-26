import { canManage, useAuth } from "@/auth/context";
import { AttributesCard } from "@/components/abac/AttributesEditor";
import { useTreeContext } from "./context";

/** v0.8 ABAC (§16.5): tree attributes (`tree.<key>` in policy conditions); editable by tree admins (tree:admin). */
export function TreeAttributesCard() {
  const { tenant, tree, summary } = useTreeContext();
  const { isAdmin, roleIn } = useAuth();
  return (
    <AttributesCard
      target={{ kind: "tree", tenant, tree }}
      editable={canManage(roleIn(summary.tree.tenant_id), isAdmin)}
      label="Tree attributes"
      description={
        <>
          Key/value pairs that policy conditions match as <span className="font-mono">tree.&lt;key&gt;</span>, e.g. classification =
          confidential. Saving replaces the whole set.
        </>
      }
    />
  );
}
