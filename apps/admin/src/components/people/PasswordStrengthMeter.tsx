import { cn } from "@/lib/utils";
import { passwordStrength } from "./password";

/** Four-segment greyscale meter; segments fill darker as strength grows. */
export function PasswordStrengthMeter({ password, id }: { password: string; id?: string }) {
  if (!password) return null;
  const { score, label } = passwordStrength(password);
  return (
    <div id={id} className="flex items-center gap-3" aria-live="polite">
      <div
        role="meter"
        aria-label="Password strength"
        aria-valuemin={0}
        aria-valuemax={4}
        aria-valuenow={score}
        aria-valuetext={label}
        className="grid flex-1 grid-cols-4 gap-1"
      >
        {[1, 2, 3, 4].map((i) => (
          <span key={i} className={cn("h-1.5 rounded-full", i <= score ? "bg-foreground" : "bg-muted")} />
        ))}
      </div>
      <span className={cn("w-16 text-right text-xs", score >= 3 ? "font-semibold" : "text-muted-foreground")}>{label}</span>
    </div>
  );
}
