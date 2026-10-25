import { Eye, EyeOff, WandSparkles } from "lucide-react";
import { useState, type ComponentProps } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { CopyButton } from "./CopyButton";
import { generatePassword } from "./password";
import { PasswordStrengthMeter } from "./PasswordStrengthMeter";

type InputProps = Omit<ComponentProps<"input">, "value" | "onChange" | "type">;

interface PasswordFieldProps extends InputProps {
  value: string;
  onChange: (value: string) => void;
  /** Offer "Generate" and "Copy" (passwords set for someone else). */
  generate?: boolean;
  /** Show the strength meter below the input. */
  meter?: boolean;
}

/**
 * Password input with a show/hide toggle, an optional strong-password
 * generator and a strength meter. Extra props (id, aria-*, name, ref…) go to
 * the <input>, so it works inside shadcn's <FormControl>.
 */
export function PasswordField({ value, onChange, generate = false, meter = false, className, ...inputProps }: PasswordFieldProps) {
  const [visible, setVisible] = useState(false);
  const Toggle = visible ? EyeOff : Eye;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Input
            {...inputProps}
            type={visible ? "text" : "password"}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            spellCheck={false}
            className={cn("pr-9 font-mono", className)}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="absolute top-1/2 right-1.5 -translate-y-1/2"
            aria-label={visible ? "Hide password" : "Show password"}
            aria-pressed={visible}
            onClick={() => setVisible((v) => !v)}
          >
            <Toggle aria-hidden="true" />
          </Button>
        </div>
        {generate && (
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              onChange(generatePassword());
              setVisible(true);
            }}
          >
            <WandSparkles aria-hidden="true" />
            Generate
          </Button>
        )}
      </div>
      {meter && <PasswordStrengthMeter password={value} />}
      {generate && value && (
        <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
          <span>Share it over a secure channel; it is not shown again after saving.</span>
          <CopyButton value={value} label="Copy password" copiedMessage="Password copied" />
        </div>
      )}
    </div>
  );
}
