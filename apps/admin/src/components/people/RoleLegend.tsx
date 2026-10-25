import { Check, Minus } from "lucide-react";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { CAPABILITIES, ROLE_SUMMARY, ROLES_ASCENDING, roleAllows } from "./roles";

/** Capability matrix: roles are cumulative, each includes everything to its left. */
export function RoleLegend() {
  return (
    <div className="flex flex-col gap-4">
      <dl className="grid gap-x-4 gap-y-1.5 text-sm sm:grid-cols-[max-content_1fr]">
        {ROLES_ASCENDING.map((r) => (
          <div key={r} className="contents">
            <dt className="font-medium capitalize">{r}</dt>
            <dd className="text-muted-foreground">{ROLE_SUMMARY[r]}</dd>
          </div>
        ))}
      </dl>
      <Table aria-label="Role permissions">
        <TableHeader>
          <TableRow>
            <TableHead>Capability</TableHead>
            {ROLES_ASCENDING.map((r) => (
              <TableHead key={r} className="text-center capitalize">
                {r}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {CAPABILITIES.map((c) => (
            <TableRow key={c.label}>
              <TableCell>{c.label}</TableCell>
              {ROLES_ASCENDING.map((r) => {
                const ok = roleAllows(r, c.minRole);
                return (
                  <TableCell key={r} className="text-center">
                    {ok ? (
                      <Check role="img" aria-label="Allowed" className="mx-auto size-4" />
                    ) : (
                      <Minus role="img" aria-label="Not allowed" className="mx-auto size-4 text-muted-foreground" />
                    )}
                  </TableCell>
                );
              })}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
