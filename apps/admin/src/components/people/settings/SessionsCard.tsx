import { LogOut } from "lucide-react";
import { useState } from "react";
import { useAuth } from "@/auth/context";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

export function SessionsCard() {
  const { signOut } = useAuth();
  const [signingOut, setSigningOut] = useState(false);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Sessions</CardTitle>
        <CardDescription>Each sign-in creates a session token stored in that browser.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
          <li>Signing out revokes this browser's session token on the server.</li>
          <li>Changing your password signs out all of your other sessions.</li>
          <li>API tokens are separate from sessions: revoke them individually above.</li>
        </ul>
        <div>
          <Button
            variant="outline"
            disabled={signingOut}
            onClick={() => {
              setSigningOut(true);
              void signOut();
            }}
          >
            <LogOut aria-hidden="true" />
            {signingOut ? "Signing out…" : "Sign out of this session"}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
