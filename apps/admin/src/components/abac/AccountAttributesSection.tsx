import { useAuth } from "@/auth/context";
import { AttributesCard } from "./AttributesEditor";

/** v0.8 ABAC (§16.5): an account's attributes in the account sheet; only server admins read and write them. */
export function AccountAttributesSection({ username }: { username: string }) {
  const { isAdmin } = useAuth();
  if (!isAdmin) return null;
  return (
    <AttributesCard
      variant="section"
      target={{ kind: "account", username }}
      editable
      label="Account attributes"
      description={
        <>
          Matched by policy conditions as <span className="font-mono">principal.&lt;key&gt;</span>.
        </>
      }
    />
  );
}
