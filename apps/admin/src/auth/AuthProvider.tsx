import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import type { AuthResponse } from "@/api/types";
import { AuthContext, type AuthContextValue } from "./context";

export function AuthProvider({ children }: { children: ReactNode }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const [token, setToken] = useState<string | null>(() => api.client.token);

  // A 401 on any authenticated call means the session is gone.
  useEffect(
    () =>
      api.client.onUnauthorized(() => {
        setToken(null);
        queryClient.clear();
      }),
    [api, queryClient],
  );

  const me = useQuery({ queryKey: keys.me, queryFn: api.auth.me, enabled: token !== null, staleTime: 60_000 });

  const signIn = useCallback(
    (res: AuthResponse) => {
      queryClient.clear();
      api.client.setToken(res.token);
      setToken(res.token);
    },
    [api, queryClient],
  );

  const signOut = useCallback(async () => {
    try {
      await api.auth.logout();
    } catch {
      // The token may already be invalid; sign out locally regardless.
    } finally {
      api.client.setToken(null);
      setToken(null);
      queryClient.clear();
    }
  }, [api, queryClient]);

  const value = useMemo<AuthContextValue>(() => {
    const memberships = me.data?.memberships ?? [];
    return {
      token,
      me,
      account: me.data?.account,
      isAdmin: me.data?.account.is_admin ?? false,
      signIn,
      signOut,
      roleIn: (tenantId) => memberships.find((m) => m.tenant.id === tenantId)?.role,
    };
  }, [token, me, signIn, signOut]);

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
