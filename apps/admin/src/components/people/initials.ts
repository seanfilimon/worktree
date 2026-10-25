/** Up to two uppercase initials: "Alice Admin" → "AA", "bob" → "BO", "acme-corp" → "AC". */
export function initials(name: string): string {
  const parts = name.trim().split(/[\s._-]+/).filter(Boolean);
  if (parts.length === 0) return "?";
  const [first = "", second] = parts;
  const chars = second ? [...first].slice(0, 1).concat([...second].slice(0, 1)) : [...first].slice(0, 2);
  return chars.join("").toUpperCase();
}
