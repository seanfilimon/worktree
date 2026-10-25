import { MIN_PASSWORD_LENGTH } from "./validation";

export interface PasswordStrength {
  /** 0 = unusable (too short), 1 = weak … 4 = strong. */
  score: 0 | 1 | 2 | 3 | 4;
  label: "Too short" | "Weak" | "Fair" | "Good" | "Strong";
}

const LABELS: PasswordStrength["label"][] = ["Too short", "Weak", "Fair", "Good", "Strong"];

/** Character classes present: lowercase, uppercase, digits, other. */
function variety(pw: string): number {
  return [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((re) => re.test(pw)).length;
}

/** A coarse length + variety estimate; the server only enforces the minimum length. */
export function passwordStrength(pw: string): PasswordStrength {
  if (pw.length < MIN_PASSWORD_LENGTH) return { score: 0, label: LABELS[0]! };
  const v = variety(pw);
  let score = 1;
  if (!/^(.)\1*$/.test(pw)) {
    if (pw.length >= 12) score += 1;
    if (v >= 3) score += 1;
    if (pw.length >= 16 && v === 4) score += 1;
  }
  const s = score as PasswordStrength["score"];
  return { score: s, label: LABELS[s]! };
}

const LOWER = "abcdefghijkmnopqrstuvwxyz";
const UPPER = "ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS = "23456789";
const SYMBOLS = "!#%+-=?@^_~";
const ALL = LOWER + UPPER + DIGITS + SYMBOLS;

/** Uniform random index in [0, n) without modulo bias. */
function randomIndex(n: number): number {
  const buf = new Uint32Array(1);
  const limit = Math.floor(0x1_0000_0000 / n) * n;
  for (;;) {
    crypto.getRandomValues(buf);
    const x = buf[0]!;
    if (x < limit) return x % n;
  }
}

function pick(chars: string): string {
  return chars.charAt(randomIndex(chars.length));
}

/**
 * Cryptographically random password with every character class present and
 * look-alike characters (0/O, 1/l/I) left out.
 */
export function generatePassword(length = 20): string {
  const chars = [pick(LOWER), pick(UPPER), pick(DIGITS), pick(SYMBOLS)];
  while (chars.length < length) chars.push(pick(ALL));
  // Fisher-Yates so the guaranteed classes are not always first.
  for (let i = chars.length - 1; i > 0; i--) {
    const j = randomIndex(i + 1);
    [chars[i], chars[j]] = [chars[j]!, chars[i]!];
  }
  return chars.join("");
}
