import { AttributesCard } from "./AttributesEditor";

/** v0.8 ABAC (§16.5): the signed-in account's own attributes, read-only. */
export function MyAttributesCard() {
  return (
    <AttributesCard
      target={{ kind: "me" }}
      editable={false}
      label="My attributes"
      title="My attributes"
      description={
        <>
          Set by server admins. Policy conditions match them as <span className="font-mono">principal.&lt;key&gt;</span>.
        </>
      }
    />
  );
}
