const e = encodeURIComponent;

export function tenantPath(tenant: string): string {
  return `/tenants/${e(tenant)}`;
}

/** `/trees/:tenant/:tree[/section]`. */
export function treePath(tenant: string, tree: string, section?: string): string {
  return `/trees/${e(tenant)}/${e(tree)}${section ? `/${section}` : ""}`;
}

export function filesPath(tenant: string, tree: string, opts: { snapshot?: string; path?: string; file?: string } = {}): string {
  const q = new URLSearchParams();
  if (opts.snapshot) q.set("snapshot", opts.snapshot);
  if (opts.path) q.set("path", opts.path);
  if (opts.file) q.set("file", opts.file);
  const s = q.toString();
  return `${treePath(tenant, tree, "files")}${s ? `?${s}` : ""}`;
}

export function mergeRequestPath(tenant: string, tree: string, n: number): string {
  return treePath(tenant, tree, `merge-requests/${n}`);
}
