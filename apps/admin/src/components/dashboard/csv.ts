export type CsvValue = string | number | boolean | null | undefined;

/** Leading characters spreadsheets treat as a formula (CSV injection). */
const FORMULA = /^[=+\-@\t\r]/;

function cell(v: CsvValue): string {
  let s = v === null || v === undefined ? "" : String(v);
  if (typeof v === "string" && FORMULA.test(s)) s = `'${s}`;
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** RFC 4180 CSV (CRLF line breaks), neutralising spreadsheet formulas in text cells. */
export function toCsv(header: readonly string[], rows: readonly (readonly CsvValue[])[]): string {
  return [header, ...rows].map((r) => r.map(cell).join(",")).join("\r\n") + "\r\n";
}

/** Saves `text` as a file through a temporary object URL. */
export function downloadText(filename: string, text: string, type = "text/csv;charset=utf-8"): void {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.append(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
