import { PageHeader } from "@/components/PageHeader";
import { AppearanceCard } from "@/components/people/settings/AppearanceCard";
import { ChangePasswordCard } from "@/components/people/settings/ChangePasswordCard";
import { ProfileCard } from "@/components/people/settings/ProfileCard";
import { MfaCard } from "@/pages/security/MfaCard";
import { SessionsCard } from "@/pages/security/SessionsCard";
import { SigningKeysCard } from "@/pages/security/SigningKeysCard";
import { TokensCard } from "@/pages/security/TokensCard";

export function SettingsPage() {
  return (
    <>
      <PageHeader
        title="Settings"
        breadcrumbs={[{ label: "Settings" }]}
        description="Your profile, password, security, tokens and preferences."
      />
      <div className="flex flex-col gap-8">
        <div className="grid gap-4 lg:grid-cols-2">
          <ProfileCard />
          <ChangePasswordCard />
        </div>
        <section aria-labelledby="settings-security" className="flex flex-col gap-4">
          <h2 id="settings-security" className="text-lg font-semibold tracking-tight">
            Security
          </h2>
          <MfaCard />
          <SessionsCard />
          <TokensCard />
          <SigningKeysCard />
        </section>
        <div className="grid gap-4 lg:grid-cols-2">
          <AppearanceCard />
        </div>
      </div>
    </>
  );
}
