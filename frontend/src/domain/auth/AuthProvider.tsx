import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import type { ReactNode } from "react";
import {
  checkSession,
  login as apiLogin,
  logout as apiLogout,
} from "@/api/auth";
import type { SessionUser } from "@/api/auth";
import { setCsrfToken, clearCsrfToken } from "@/api/client";

/** Authentication state. */
export type AuthState =
  | { status: "loading" }
  | { status: "authenticated"; user: SessionUser }
  | { status: "unauthenticated" };

/** Context value exposed to consumers. */
export interface AuthContextValue {
  /** Current authentication state. */
  auth: AuthState;
  /** Initiate the OIDC login flow (redirects the browser). */
  login: () => Promise<void>;
  /** Log out and clear session. */
  logout: () => Promise<void>;
  /** Mark the user as authenticated after a successful callback. */
  setAuthenticated: (user: SessionUser, csrfToken: string) => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

interface AuthProviderProps {
  children: ReactNode;
}

export function AuthProvider({ children }: AuthProviderProps) {
  const [auth, setAuth] = useState<AuthState>({ status: "loading" });

  // Check session on mount
  useEffect(() => {
    let cancelled = false;

    async function verify() {
      const user = await checkSession();
      if (cancelled) return;

      if (user) {
        // Recover CSRF token from session check (handles page refresh)
        setCsrfToken(user.csrf_token);
        setAuth({ status: "authenticated", user });
      } else {
        setAuth({ status: "unauthenticated" });
      }
    }

    void verify();
    return () => {
      cancelled = true;
    };
  }, []);

  const login = useCallback(async () => {
    const response = await apiLogin();
    // Redirect to the OIDC provider
    window.location.href = response.authorization_url;
  }, []);

  const logout = useCallback(async () => {
    await apiLogout();
    clearCsrfToken();
    setAuth({ status: "unauthenticated" });
  }, []);

  const setAuthenticated = useCallback(
    (user: SessionUser, csrfToken: string) => {
      setCsrfToken(csrfToken);
      setAuth({ status: "authenticated", user });
    },
    [],
  );

  return (
    <AuthContext.Provider value={{ auth, login, logout, setAuthenticated }}>
      {children}
    </AuthContext.Provider>
  );
}

/**
 * Hook to access authentication state and actions.
 * Must be used within an AuthProvider.
 */
export function useAuth(): AuthContextValue {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error("useAuth must be used within an AuthProvider");
  }
  return context;
}
