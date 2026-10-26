/** Comment thread grouping and mutations shared by the Discussion and the inline diff review (§13.4, §15.7). */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTreeV05Api, v05Keys } from "@/api/v05tree";
import { useV07ReviewApi, type CreateCommentRequestV07, type ReviewCommentV07 } from "@/api/v07review";

export interface Thread {
  key: string;
  path?: string;
  line?: number;
  comments: ReviewCommentV07[];
}

/** General comments first, then file threads by path and line; each thread oldest first. */
export function groupThreads(comments: readonly ReviewCommentV07[]): Thread[] {
  const map = new Map<string, Thread>();
  const sorted = [...comments].sort((a, b) => a.created_at.localeCompare(b.created_at));
  for (const c of sorted) {
    if (!c.path) {
      // Every general comment is its own thread.
      map.set(`general:${c.id}`, { key: `general:${c.id}`, comments: [c] });
      continue;
    }
    const key = `file:${c.path}:${c.line ?? ""}`;
    const t = map.get(key);
    if (t) t.comments.push(c);
    else map.set(key, { key, path: c.path, line: c.line || undefined, comments: [c] });
  }
  const threads = [...map.values()];
  const general = threads.filter((t) => !t.path);
  const files = threads
    .filter((t) => t.path)
    .sort((a, b) => a.path!.localeCompare(b.path!) || (a.line ?? 0) - (b.line ?? 0));
  return [...general, ...files];
}

export function threadLabel(t: Thread): string {
  if (!t.path) return "General comment";
  return t.line ? `${t.path}:${t.line}` : t.path;
}

export function useCommentMutations(tenant: string, tree: string, n: number) {
  const api = useTreeV05Api();
  const v07 = useV07ReviewApi();
  const queryClient = useQueryClient();
  const invalidate = () => queryClient.invalidateQueries({ queryKey: v05Keys.mrComments(tenant, tree, n) });
  const add = useMutation({
    // v0.7 (§15.7): the body may carry a suggestion.
    mutationFn: (body: CreateCommentRequestV07) => v07.comments.add(tenant, tree, n, body),
    meta: { inlineError: true },
    onSuccess: invalidate,
  });
  const resolve = useMutation({
    mutationFn: ({ id, resolved }: { id: string; resolved: boolean }) => api.reviews.updateComment(tenant, tree, n, id, { resolved }),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.reviews.removeComment(tenant, tree, n, id),
    onSuccess: invalidate,
  });
  return { add, resolve, remove };
}

export type CommentMutations = ReturnType<typeof useCommentMutations>;
