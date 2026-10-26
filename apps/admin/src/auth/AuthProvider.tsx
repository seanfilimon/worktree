import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import type { AuthResponse } from "@/api/types";
import { AuthContext, type AuthContextValue } from "./context";
import { clearSsoHash, readSsoHash } from "./ssoHash";

export function AuthProvider({ children }: { children: ReactNode }) {
  const api = useApi();
  const queryClient = useQueryClient();
  // An SSO round trip lands with `#sso_token=` / `#sso_error=`. Reading is
  // side-effect free apart from storing the token (idempotent), so it is safe
  // in a (possibly double-invoked) state initializer; the fragment itself is
  // stripped in the effect below.
  const [sso] = useState(() => {
    const result = readSsoHash();
    if (result.token) api.client.setToken(result.token);
    return result;
  });
  const [token, setToken] = useState<string | null>(() => sso.token ?? api.client.token);
  const [ssoError, setSsoError] = useState<string | null>(sso.error);

  useEffect(() => {
    if (sso.token || sso.error) clearSsoHash();
  }, [sso]);

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
      api.client.setRefreshToken(res.refresh_token ?? null); // v0.8 device-auth (§16.6)
      setToken(res.token);
      setSsoError(null);
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

  const clearSsoError = useCallback(() => setSsoError(null), []);

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
      ssoError,
      clearSsoError,
    };
  }, [token, me, signIn, signOut, ssoError, clearSsoError]);

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
