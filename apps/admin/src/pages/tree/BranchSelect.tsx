import { useBranches } from "@/api/queries";
import { SelectField } from "@/components/legacy/Field";

interface BranchSelectProps {
  tenant: string;
  tree: string;
  value: string;
  onChange: (branch: string) => void;
  label?: string;
  /** Adds an "All branches" option with value "". */
  allowAll?: boolean;
  className?: string;
}

export function BranchSelect({ tenant, tree, value, onChange, label = "Branch", allowAll = false, className = "w-48" }: BranchSelectProps) {
  const branches = useBranches(tenant, tree);
  const names = (branches.data ?? []).map((b) => b.name);
  if (value && !names.includes(value)) names.unshift(value);
  return (
    <SelectField label={label} hideLabel value={value} onChange={(e) => onChange(e.target.value)} className={className} disabled={branches.isPending}>
      {allowAll && <option value="">All branches</option>}
      {names.map((n) => (
        <option key={n} value={n}>
          {n}
        </option>
      ))}
    </SelectField>
  );
}
