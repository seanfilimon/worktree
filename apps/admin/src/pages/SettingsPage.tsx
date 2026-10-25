import { PageHeader } from "@/components/PageHeader";
import { AppearanceCard } from "@/components/people/settings/AppearanceCard";
import { ChangePasswordCard } from "@/components/people/settings/ChangePasswordCard";
import { ProfileCard } from "@/components/people/settings/ProfileCard";
import { SessionsCard } from "@/components/people/settings/SessionsCard";
import { TokensCard } from "@/components/people/settings/TokensCard";

export function SettingsPage() {
  return (
    <>
      <PageHeader title="Settings" breadcrumbs={[{ label: "Settings" }]} description="Your profile, password, tokens and preferences." />
      <div className="grid gap-4 lg:grid-cols-2">
        <ProfileCard />
        <ChangePasswordCard />
        <div className="lg:col-span-2">
          <TokensCard />
        </div>
        <AppearanceCard />
        <SessionsCard />
      </div>
    </>
  );
}
