import { createContext, useContext } from "react";
import type { Api } from "./endpoints";

export const ApiContext = createContext<Api | null>(null);

export function useApi(): Api {
  const api = useContext(ApiContext);
  if (!api) throw new Error("useApi must be used inside <AppProviders>");
  return api;
}
