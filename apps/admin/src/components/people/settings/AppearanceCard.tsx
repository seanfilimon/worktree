import { Monitor, Moon, Sun } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { THEME_PREFERENCES, useTheme, type ThemePreference } from "@/theme/theme";

const ICONS: Record<ThemePreference, typeof Sun> = { light: Sun, dark: Moon, system: Monitor };

function isPreference(v: string): v is ThemePreference {
  return (THEME_PREFERENCES as readonly string[]).includes(v);
}

export function AppearanceCard() {
  const { preference, setPreference } = useTheme();
  return (
    <Card>
      <CardHeader>
        <CardTitle id="appearance-title">Appearance</CardTitle>
        <CardDescription>“System” follows your operating system's light or dark setting.</CardDescription>
      </CardHeader>
      <CardContent>
        <ToggleGroup
          type="single"
          variant="outline"
          aria-labelledby="appearance-title"
          value={preference}
          // Radix reports "" when the active item is clicked again; keep the current choice.
          onValueChange={(v) => isPreference(v) && setPreference(v)}
        >
          {THEME_PREFERENCES.map((p) => {
            const Icon = ICONS[p];
            return (
              <ToggleGroupItem key={p} value={p} className="capitalize">
                <Icon aria-hidden="true" />
                {p}
              </ToggleGroupItem>
            );
          })}
        </ToggleGroup>
      </CardContent>
    </Card>
  );
}
