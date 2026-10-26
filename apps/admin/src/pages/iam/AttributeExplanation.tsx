import { Check, X } from "lucide-react";
import { ATTRIBUTE_SCOPES, type Attributes } from "@/api/abac";
import type { Decision } from "@/api/iam";
import { conditionText } from "@/components/abac/abac";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";

const SCOPE_LABEL = { principal: "Principal", tree: "Tree", tenant: "Tenant" } as const;

/**
 * v0.8 ABAC (§16.5): the attributes a simulated evaluation saw (after
 * overrides) and every attribute condition it checked. Renders nothing when
 * the decision carries neither.
 */
export function AttributeExplanation({ decision }: { decision: Decision }) {
  const attrs = decision.attributes;
  const conditions = decision.conditions ?? [];
  const scopes = ATTRIBUTE_SCOPES.map((s) => [s, attrs?.[s] ?? {}] as const);
  const anyAttrs = scopes.some(([, a]) => Object.keys(a).length > 0);
  if (!anyAttrs && conditions.length === 0) return null;

  return (
    <Card aria-label="Attribute evaluation">
      <CardHeader>
        <CardTitle>Attributes</CardTitle>
        <CardDescription>What the evaluation saw, after overrides, and how each attribute condition fared.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        <dl className="grid gap-4 sm:grid-cols-3">
          {scopes.map(([scope, a]) => (
            <div key={scope} className="grid content-start gap-1.5">
              <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{SCOPE_LABEL[scope]}</dt>
              <dd>
                <ScopeAttributes scope={scope} attributes={a} />
              </dd>
            </div>
          ))}
        </dl>
        {conditions.length > 0 && (
          <div className="grid gap-2">
            <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Conditions ({conditions.length})</h3>
            <div className="overflow-x-auto rounded-md border">
              <Table aria-label="Attribute conditions">
                <TableHeader>
                  <TableRow>
                    <TableHead>Policy</TableHead>
                    <TableHead>Statement</TableHead>
                    <TableHead>Effect</TableHead>
                    <TableHead>Condition</TableHead>
                    <TableHead>Actual</TableHead>
                    <TableHead className="text-right">Result</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {conditions.map((c, i) => (
                    <TableRow key={i}>
                      <TableCell className="font-mono text-xs">{c.policy}</TableCell>
                      <TableCell className="font-mono text-xs">{c.statement}</TableCell>
                      <TableCell>
                        <Badge variant={c.effect === "deny" ? "default" : "outline"} className="uppercase">
                          {c.effect}
                        </Badge>
                      </TableCell>
                      <TableCell className="font-mono text-xs whitespace-normal break-all">{conditionText(c)}</TableCell>
                      <TableCell className="font-mono text-xs break-all">
                        {c.value === null ? <span className="text-muted-foreground italic">unset</span> : c.value}
                      </TableCell>
                      <TableCell className="text-right">
                        {c.matched ? (
                          <Badge>
                            <Check aria-hidden="true" />
                            Matched
                          </Badge>
                        ) : (
                          <Badge variant="outline">
                            <X aria-hidden="true" />
                            Not met
                          </Badge>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ScopeAttributes({ scope, attributes }: { scope: string; attributes: Attributes }) {
  const keys = Object.keys(attributes).sort();
  if (keys.length === 0) return <span className="text-sm text-muted-foreground">None</span>;
  return (
    <ul aria-label={`${scope} attributes`} className="grid gap-1">
      {keys.map((k) => (
        <li key={k} className="font-mono text-xs break-all">
          <span className="text-muted-foreground">{k}</span> = {attributes[k]}
        </li>
      ))}
    </ul>
  );
}
