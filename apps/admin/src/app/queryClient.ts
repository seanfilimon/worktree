import { MutationCache, QueryClient } from "@tanstack/react-query";
import { isApiError } from "@/api/client";

declare module "@tanstack/react-query" {
  interface Register {
    mutationMeta: {
      /** The component renders the error itself; skip the global toast. */
      inlineError?: boolean;
    };
  }
}

interface QueryClientOptions {
  onMutationError?: (error: unknown) => void;
  /** Disable retries (tests). */
  retry?: boolean;
}

export function createQueryClient({ onMutationError, retry = true }: QueryClientOptions = {}): QueryClient {
  return new QueryClient({
    mutationCache: new MutationCache({
      onError: (error, _vars, _ctx, mutation) => {
        if (!mutation.meta?.inlineError) onMutationError?.(error);
      },
    }),
    defaultOptions: {
      queries: {
        staleTime: 15_000,
        refetchOnWindowFocus: retry,
        // Client errors (4xx) will not succeed on retry.
        retry: retry ? (count, error) => !(isApiError(error) && error.status >= 400 && error.status < 500) && count < 2 : false,
      },
      mutations: { retry: false },
    },
  });
}
